/** @jsxImportSource solid-js */
import { useNavigate } from '@solidjs/router'
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

import { decideReview, getReviewFile, type ReviewFile, type ReviewHunk } from '../../api/review'
import { sessionDisplayTitle } from '../../api/sessions'
import type { ReviewView } from './diffView'
import {
  hunkAttachment,
  hunkSlice,
  rejectReasonInstruction,
  wholeFileAttachment
} from './reviewAttachment'
import { buildFileTree, filesInTreeOrder } from './tree'
import type { FileAttachment } from '../chat/textAttachments'
import styles from './ReviewScreen.module.css'
import { chat } from '../../state/chatStore'
import { workspaces } from '../../state/workspaceStore'
import { useSessionReview } from './useSessionReview'
import { DiffList } from './DiffList'
import { FileTree } from './FileTree'
import { RejectReasonDialog } from './RejectReasonDialog'

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

export function ReviewScreen(props: { sessionId: string }) {
  const navigate = useNavigate()
  const session = () => chat.sessions.find((item) => item.id === props.sessionId) ?? null
  const root = () => {
    const current = session()
    return current
      ? (workspaces.workspaces.find((item) => item.id === current.workspace_id)?.root ?? null)
      : null
  }
  const review = useSessionReview(() => props.sessionId)
  const [view, setView] = createSignal<ReviewView>('diff')
  const [pendingKey, setPendingKey] = createSignal<string | null>(null)
  const [decideError, setDecideError] = createSignal<string | null>(null)
  const [reasonTarget, setReasonTarget] = createSignal<{
    path: string
    hunkIds?: string[]
    range: { start: number; end: number } | null
  } | null>(null)
  const [reasonBusy, setReasonBusy] = createSignal(false)
  const [reasonError, setReasonError] = createSignal<string | null>(null)
  const [selected, setSelected] = createSignal<string | null>(null)
  const [hiddenPaths, setHiddenPaths] = createSignal<string[]>([])
  const [bodies, setBodies] = createSignal<Record<string, ReviewFile>>({})
  const [bodyError, setBodyError] = createSignal<string | null>(null)

  createEffect(() => {
    const loaded = review.files()
    setHiddenPaths((current) => current.filter((path) => loaded.some((file) => file.path === path)))
  })

  const files = createMemo(() =>
    review.files().filter((file) => !hiddenPaths().includes(file.path))
  )
  const orderedPaths = createMemo(() => filesInTreeOrder(files().map((file) => file.path)))
  const tree = createMemo(() => buildFileTree(orderedPaths()))
  const active = () => {
    const current = selected()
    const paths = orderedPaths()
    return current && paths.includes(current) ? current : (paths[0] ?? null)
  }
  const openBody = () => {
    const path = active()
    return path ? (bodies()[path] ?? null) : null
  }

  createEffect(() => {
    const path = active()
    void (chat.reviewTickBySession[props.sessionId] ?? 0)
    setBodyError(null)
    if (!path) {
      return
    }
    let cancelled = false
    void getReviewFile(props.sessionId, path)
      .then((file) => {
        if (cancelled) {
          return
        }
        if (!file) {
          setHiddenPaths((current) => (current.includes(path) ? current : [...current, path]))
          return
        }
        setBodies((current) => ({ ...current, [file.path]: file }))
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setBodyError(err instanceof Error ? err.message : 'Failed to load file')
        }
      })
    onCleanup(() => {
      cancelled = true
    })
  })

  async function reload(path: string) {
    const next = await getReviewFile(props.sessionId, path)
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
      await decideReview(props.sessionId, path, 'approve')
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

  async function decideWholeFile(path: string, decision: 'approve' | 'reject') {
    if (decision === 'approve') {
      await approveFile(path)
      return
    }
    setPendingKey(path)
    setDecideError(null)
    try {
      await decideReview(props.sessionId, path, decision)
      await reload(path)
    } catch (err: unknown) {
      setDecideError(err instanceof Error ? err.message : 'Failed to update review')
    } finally {
      setPendingKey(null)
    }
  }

  async function decide(path: string, decision: 'approve' | 'reject', hunkIds?: string[]) {
    if (!hunkIds || hunkIds.length === 0) {
      await decideWholeFile(path, decision)
      return
    }
    const ordered = orderHunks(bodies()[path]?.hunks ?? [], hunkIds, decision)
    if (ordered.length === 0) {
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
        await decideReview(props.sessionId, path, decision, id)
      }
      await reload(path)
    } catch (err: unknown) {
      setDecideError(err instanceof Error ? err.message : 'Failed to update review')
    } finally {
      setPendingKey(null)
    }
  }

  function openRejectWithReason(path: string, hunkIds?: string[]) {
    const body = bodies()[path]
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
    const target = reasonTarget()
    if (!target) {
      return
    }
    const { path, hunkIds } = target
    const body = bodies()[path]
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
        attachment = hunkAttachment(body, hunk, root())
        const slice = hunkSlice(body, hunk)
        range = { start: slice.startLine, end: slice.endLine }
      } else {
        attachment = wholeFileAttachment(body, root())
      }
    } catch (err: unknown) {
      setReasonError(err instanceof Error ? err.message : 'Failed to attach the file')
      return
    }
    setReasonBusy(true)
    setReasonError(null)
    try {
      await decideReview(props.sessionId, path, 'reject', hunk?.id)
    } catch (err: unknown) {
      setReasonBusy(false)
      setReasonError(err instanceof Error ? err.message : 'Failed to update review')
      return
    }
    setReasonBusy(false)
    setReasonTarget(null)
    navigate(`/sessions/${props.sessionId}`)
    void chat.sendInstruction(rejectReasonInstruction(path, range, reason), undefined, [attachment])
  }

  const shown = () => {
    const body = openBody()
    return body ? [body] : []
  }

  return (
    <div class={styles.page}>
      <header class={styles.header}>
        <button
          type="button"
          class={styles.back}
          onClick={() => navigate(`/sessions/${props.sessionId}`)}
        >
          ← {sessionDisplayTitle(session())}
        </button>
        <div class={styles.views} role="radiogroup" aria-label="Diff view">
          <For each={VIEWS}>
            {(item) => (
              <button
                type="button"
                role="radio"
                aria-checked={view() === item.id}
                class={view() === item.id ? styles.viewActive : styles.view}
                onClick={() => setView(item.id)}
              >
                {item.label}
              </button>
            )}
          </For>
        </div>
      </header>
      <Show when={review.error()}>
        <p class={styles.message} role="alert">
          {review.error()}
        </p>
      </Show>
      <Show when={!review.error() && review.loading() && files().length === 0}>
        <p class={styles.message}>Loading review…</p>
      </Show>
      <Show when={!review.error() && (decideError() || bodyError())}>
        <p class={styles.message} role="alert">
          {decideError() ?? bodyError()}
        </p>
      </Show>
      <Show
        when={
          !review.error() && !(review.loading() && files().length === 0) && files().length === 0
        }
      >
        <p class={styles.message}>No files changed in this session.</p>
      </Show>
      <Show
        when={!review.error() && !(review.loading() && files().length === 0) && files().length > 0}
      >
        <div class={styles.body}>
          <FileTree nodes={tree()} selected={active()} onSelect={setSelected} />
          <div class={styles.diffs}>
            <Show
              when={openBody() || bodyError()}
              fallback={<p class={styles.message}>Loading file…</p>}
            >
              <Show when={openBody()}>
                <DiffList
                  files={shown()}
                  view={view()}
                  pendingKey={pendingKey()}
                  onDecide={(path, decision, hunkIds) => {
                    void decide(path, decision, hunkIds)
                  }}
                  onRejectWithReason={openRejectWithReason}
                />
              </Show>
            </Show>
          </div>
        </div>
      </Show>
      <Show when={reasonTarget()} keyed>
        {(target) => (
          <RejectReasonDialog
            open
            path={target.path}
            range={target.range}
            busy={reasonBusy()}
            error={reasonError()}
            onCancel={() => {
              if (reasonBusy()) {
                return
              }
              setReasonTarget(null)
              setReasonError(null)
            }}
            onSubmit={(reason) => void submitReason(reason)}
          />
        )}
      </Show>
    </div>
  )
}
