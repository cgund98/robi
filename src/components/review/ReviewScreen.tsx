import { useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { decideReview } from '../../api/review'
import { sessionDisplayTitle } from '../../api/sessions'
import { useSessionReview } from '../../app/useSessionReview'
import { useChatStore } from '../../state/chatStore'
import { DiffList } from './DiffList'
import type { ReviewView } from './diffView'
import { buildFileTree, filesInTreeOrder } from './tree'
import { FileTree } from './FileTree'
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
  const { files: loaded, error, loading } = useSessionReview(sessionId)
  const bumpReview = useChatStore((state) => state.bumpReview)
  const [view, setView] = useState<ReviewView>('diff')
  const [pendingKey, setPendingKey] = useState<string | null>(null)
  const [decideError, setDecideError] = useState<string | null>(null)
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
  const anchors = useRef(new Map<string, HTMLElement>())
  const orderedPaths = useMemo(() => filesInTreeOrder(files.map((file) => file.path)), [files])
  const orderedFiles = useMemo(() => {
    const byPath = new Map(files.map((file) => [file.path, file]))
    return orderedPaths.flatMap((path) => {
      const file = byPath.get(path)
      return file ? [file] : []
    })
  }, [files, orderedPaths])
  const tree = useMemo(() => buildFileTree(orderedPaths), [orderedPaths])
  const active = selected && orderedPaths.includes(selected) ? selected : (orderedPaths[0] ?? null)

  function register(path: string, node: HTMLElement | null) {
    if (node) {
      anchors.current.set(path, node)
    } else {
      anchors.current.delete(path)
    }
  }

  async function decide(path: string, decision: 'approve' | 'reject', hunkIds?: string[]) {
    const file = files.find((item) => item.path === path)
    const ordered = orderHunks(file?.hunks ?? [], hunkIds, decision)
    if (decision === 'approve' && ordered.length === 0) {
      await approveFile(path)
      return
    }
    setPendingKey(hunkIds ? `${path}:${hunkIds[0]}` : path)
    setDecideError(null)
    try {
      if (ordered.length === 0) {
        await decideReview(sessionId, path, decision)
      } else {
        for (const id of ordered) {
          await decideReview(sessionId, path, decision, id)
        }
      }
      bumpReview(sessionId)
    } catch (err: unknown) {
      setDecideError(err instanceof Error ? err.message : 'Failed to update review')
    } finally {
      setPendingKey(null)
    }
  }

  async function approveFile(path: string) {
    setHiddenPaths((current) => (current.includes(path) ? current : [...current, path]))
    setDecideError(null)
    try {
      await decideReview(sessionId, path, 'approve')
      bumpReview(sessionId)
    } catch (err: unknown) {
      setHiddenPaths((current) => current.filter((item) => item !== path))
      setDecideError(err instanceof Error ? err.message : 'Failed to update review')
    }
  }

  function selectFile(path: string) {
    setSelected(path)
    anchors.current.get(path)?.scrollIntoView({ block: 'start' })
  }

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <button type="button" className={styles.back} onClick={() => navigate('/')}>
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
      ) : decideError ? (
        <p className={styles.message} role="alert">
          {decideError}
        </p>
      ) : null}
      {error ? null : loading && files.length === 0 ? null : files.length === 0 ? (
        <p className={styles.message}>No files changed in this session.</p>
      ) : (
        <div className={styles.body}>
          <FileTree nodes={tree} selected={active} onSelect={selectFile} />
          <div className={styles.diffs}>
            <DiffList
              files={orderedFiles}
              view={view}
              pendingKey={pendingKey}
              register={register}
              onDecide={(path, decision, hunkIds) => {
                void decide(path, decision, hunkIds)
              }}
            />
          </div>
        </div>
      )}
    </div>
  )
}
