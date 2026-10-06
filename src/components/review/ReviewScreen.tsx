import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { decideReview, getReviewFile, type ReviewFile, type ReviewHunk } from '../../api/review'
import { sessionDisplayTitle } from '../../api/sessions'
import { useSessionReview } from '../../app/useSessionReview'
import { useChatStore } from '../../state/chatStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import type { FileAttachment } from '../chat/textAttachments'
import { DiffList } from './DiffList'
import type { ReviewView } from './diffView'
import { buildFileTree, filesInTreeOrder } from './tree'
import { FileTree } from './FileTree'
import { RejectReasonDialog } from './RejectReasonDialog'
import {
  hunkAttachment,
  hunkSlice,
  rejectReasonInstruction,
  wholeFileAttachment
} from './reviewAttachment'
import styles from './ReviewScreen.module.css'

function orderHunks(
  hunks: { id: string; old_start: number; new_start: number }[],
  ids: string[] | undefined,
  decision: 'approve' | 'reject'
): string[] {
  if (!ids || ids.length === 0) {
    return []
  }
  const chosen = hunks.filter((hunk) => ids.includes(hunk.id))
  chosen.sort((left, right) =>
    decision === 'approve' ? right.old_start - left.old_start : right.new_start - left.new_start
  )
  return chosen.map((hunk) => hunk.id)
}

const VIEWS: { id: ReviewView; label: string }[] = [
  { id: 'diff', label: 'Diff' },
  { id: 'current', label: 'Current' },
  { id: 'previous', label: 'Previous' }
]

type ReviewScreenProps = {
  sessionId: string
}

export function ReviewScreen({ sessionId }: ReviewScreenProps) {
  const navigate = useNavigate()
  const sessions = useChatStore((state) => state.sessions)
  const session = sessions.find((item) => item.id === sessionId) ?? null
  const sendInstruction = useChatStore((state) => state.sendInstruction)
  const workspaces = useWorkspaceStore((state) => state.workspaces)
  const root = session
    ? (workspaces.find((item) => item.id === session.workspace_id)?.root ?? null)
    : null
  const { files: loaded, error, loading } = useSessionReview(sessionId)
  const reviewTick = useChatStore((state) => state.reviewTickBySession[sessionId] ?? 0)
  const [view, setView] = useState<ReviewView>('diff')
  const [pendingKey, setPendingKey] = useState<string | null>(null)
  const [decideError, setDecideError] = useState<string | null>(null)
  const [reasonTarget, setReasonTarget] = useState<{
    path: string
    hunkIds?: string[]
    range: { start: number; end: number } | null
  } | null>(null)
  const [reasonBusy, setReasonBusy] = useState(false)
  const [reasonError, setReasonError] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [hiddenPaths, setHiddenPaths] = useState<string[]>([])
  const [hiddenFor, setHiddenFor] = useState(loaded)
  if (hiddenFor !== loaded) {
    setHiddenFor(loaded)
    setHiddenPaths((current) => current.filter((path) => loaded.some((file) => file.path === path)))
  }
  const files = useMemo(
    () => loaded.filter((file) => !hiddenPaths.includes(file.path)),
    [loaded, hiddenPaths]
  )
  const [bodies, setBodies] = useState<Record<string, ReviewFile>>({})
  const [bodyError, setBodyError] = useState<string | null>(null)
  const [bodyErrorFor, setBodyErrorFor] = useState<string | null>(null)
  const orderedPaths = useMemo(() => filesInTreeOrder(files.map((file) => file.path)), [files])
  const tree = useMemo(() => buildFileTree(orderedPaths), [orderedPaths])
  const active = selected && orderedPaths.includes(selected) ? selected : (orderedPaths[0] ?? null)
  const errorKey = active ? `${sessionId}:${active}:${reviewTick}` : null
  if (bodyErrorFor !== errorKey) {
    setBodyErrorFor(errorKey)
    setBodyError(null)
  }
  const openBody = active ? (bodies[active] ?? null) : null
  const shown = useMemo(() => (openBody ? [openBody] : []), [openBody])

  useEffect(() => {
    if (!active) {
      return
    }
    let cancelled = false
    void getReviewFile(sessionId, active)
      .then((file) => {
        if (cancelled) {
          return
        }
        if (!file) {
          setHiddenPaths((current) => (current.includes(active) ? current : [...current, active]))
          return
        }
        setBodies((current) => ({ ...current, [file.path]: file }))
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setBodyError(err instanceof Error ? err.message : 'Failed to load file')
        }
      })
    return () => {
      cancelled = true
    }
  }, [sessionId, active, reviewTick])

  async function decide(path: string, decision: 'approve' | 'reject', hunkIds?: string[]) {
    if (!hunkIds || hunkIds.length === 0) {
      await decideWholeFile(path, decision)
      return
    }
    const ordered = orderHunks(bodies[path]?.hunks ?? [], hunkIds, decision)
    if (ordered.length === 0) {
      // The body is stale and no longer carries this hunk. Never fall back to a
      // whole-file decision from an in-line control.
      setDecideError('This change no longer matches the file.')
      try {
        await reload(path)
      } catch (err: unknown) {
        setDecideError(err instanceof Error ? err.message : 'Failed to update review')
      }
      return
    }
    setPendingKey(`${path}:${ordered[0]}`)
    setDecideError(null)
    try {
      for (const id of ordered) {
        await decideReview(sessionId, path, decision, id)
      }
      await reload(path)
    } catch (err: unknown) {
      setDecideError(err instanceof Error ? err.message : 'Failed to update review')
    } finally {
      setPendingKey(null)
    }
  }

  async function decideWholeFile(path: string, decision: 'approve' | 'reject') {
    if (decision === 'approve') {
      await approveFile(path)
      return
    }
    setPendingKey(path)
    setDecideError(null)
    try {
      await decideReview(sessionId, path, decision)
      await reload(path)
    } catch (err: unknown) {
      setDecideError(err instanceof Error ? err.message : 'Failed to update review')
    } finally {
      setPendingKey(null)
    }
  }

  async function reload(path: string) {
    const next = await getReviewFile(sessionId, path)
    if (!next) {
      setHiddenPaths((current) => (current.includes(path) ? current : [...current, path]))
      setBodies((current) => {
        const copy = { ...current }
        delete copy[path]
        return copy
      })
    } else {
      setBodies((current) => ({ ...current, [path]: next }))
    }
  }

  async function approveFile(path: string) {
    setHiddenPaths((current) => (current.includes(path) ? current : [...current, path]))
    setDecideError(null)
    try {
      await decideReview(sessionId, path, 'approve')
      setBodies((current) => {
        const copy = { ...current }
        delete copy[path]
        return copy
      })
    } catch (err: unknown) {
      setHiddenPaths((current) => current.filter((item) => item !== path))
      setDecideError(err instanceof Error ? err.message : 'Failed to update review')
    }
  }

  function openRejectWithReason(path: string, hunkIds?: string[]) {
    const body = bodies[path]
    let range: { start: number; end: number } | null = null
    if (body && hunkIds && hunkIds.length > 0) {
      const hunk = body.hunks.find((item) => item.id === hunkIds[0])
      if (hunk) {
        const slice = hunkSlice(body, hunk)
        range = { start: slice.startLine, end: slice.endLine }
      }
    }
    setReasonError(null)
    setReasonBusy(false)
    setReasonTarget({ path, hunkIds, range })
  }

  async function submitReason(reason: string) {
    if (!reasonTarget) {
      return
    }
    const { path, hunkIds } = reasonTarget
    const body = bodies[path]
    if (!body) {
      setReasonError('This change is no longer available.')
      return
    }
    let hunk: ReviewHunk | undefined
    if (hunkIds && hunkIds.length > 0) {
      hunk = body.hunks.find((item) => item.id === hunkIds[0])
      if (!hunk) {
        setReasonError('This change no longer matches the file.')
        return
      }
    }
    let attachment: FileAttachment
    let range: { start: number; end: number } | null = null
    try {
      if (hunk) {
        attachment = hunkAttachment(body, hunk, root)
        const slice = hunkSlice(body, hunk)
        range = { start: slice.startLine, end: slice.endLine }
      } else {
        attachment = wholeFileAttachment(body, root)
      }
    } catch (err: unknown) {
      setReasonError(err instanceof Error ? err.message : 'Failed to attach the file')
      return
    }
    setReasonBusy(true)
    setReasonError(null)
    try {
      await decideReview(sessionId, path, 'reject', hunk?.id)
    } catch (err: unknown) {
      setReasonBusy(false)
      setReasonError(err instanceof Error ? err.message : 'Failed to update review')
      return
    }
    setReasonBusy(false)
    setReasonTarget(null)
    navigate(`/sessions/${sessionId}`)
    void sendInstruction(rejectReasonInstruction(path, range, reason), undefined, [attachment])
  }

  function selectFile(path: string) {
    setSelected(path)
  }

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <button
          type="button"
          className={styles.back}
          onClick={() => navigate(`/sessions/${sessionId}`)}
        >
          ← {sessionDisplayTitle(session)}
        </button>
        <div className={styles.views} role="radiogroup" aria-label="Diff view">
          {VIEWS.map((item) => (
            <button
              key={item.id}
              type="button"
              role="radio"
              aria-checked={view === item.id}
              className={view === item.id ? styles.viewActive : styles.view}
              onClick={() => setView(item.id)}
            >
              {item.label}
            </button>
          ))}
        </div>
      </header>
      {error ? (
        <p className={styles.message} role="alert">
          {error}
        </p>
      ) : loading && files.length === 0 ? (
        <p className={styles.message}>Loading review…</p>
      ) : decideError || bodyError ? (
        <p className={styles.message} role="alert">
          {decideError ?? bodyError}
        </p>
      ) : null}
      {error ? null : loading && files.length === 0 ? null : files.length === 0 ? (
        <p className={styles.message}>No files changed in this session.</p>
      ) : (
        <div className={styles.body}>
          <FileTree nodes={tree} selected={active} onSelect={selectFile} />
          <div className={styles.diffs}>
            {!openBody && !bodyError ? (
              <p className={styles.message}>Loading file…</p>
            ) : openBody ? (
              <DiffList
                files={shown}
                view={view}
                pendingKey={pendingKey}
                onDecide={(path, decision, hunkIds) => {
                  void decide(path, decision, hunkIds)
                }}
                onRejectWithReason={openRejectWithReason}
              />
            ) : null}
          </div>
        </div>
      )}
      {reasonTarget ? (
        <RejectReasonDialog
          key={`${reasonTarget.path}:${reasonTarget.hunkIds?.join(',') ?? ''}`}
          open
          path={reasonTarget.path}
          range={reasonTarget.range}
          busy={reasonBusy}
          error={reasonError}
          onCancel={() => {
            if (reasonBusy) {
              return
            }
            setReasonTarget(null)
            setReasonError(null)
          }}
          onSubmit={(reason) => void submitReason(reason)}
        />
      ) : null}
    </div>
  )
}
