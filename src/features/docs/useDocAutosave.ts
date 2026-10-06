/**
 * Debounced autosave for the docs editor.
 *
 * One second after the last keystroke, the accumulated CodeMirror change set
 * goes to `PUT .../docs/{path}`. The server reconciles it against the disk. A
 * base version it no longer has is a `409`, and the whole buffer is re-sent.
 *
 * State, per open document:
 *
 * - `base` — the version the pending changes were built from.
 * - `pending` — the change set accumulated since `base`.
 * - `full` — send the whole buffer next (a create, or after a `409`).
 * - `inflight` — one request at a time; input during a flight keeps
 *   accumulating and goes out on the next debounce.
 */
import { ChangeSet } from '@codemirror/state'
import { createSignal, onCleanup } from 'solid-js'

import { putDoc } from '../../api/docs'
import { serializeChangeSet } from './docsEditor'

/** Idle time before a save. Every keystroke resets it. */
export const AUTOSAVE_DELAY_MS = 1000

/** The first retry waits the debounce; each later one doubles, to this cap. */
const MAX_BACKOFF_MS = 8000

export type SaveStatus =
  | { kind: 'idle' }
  | { kind: 'saving' }
  | { kind: 'saved'; outcome: string }
  | { kind: 'out-of-sync' }
  | { kind: 'error'; message: string }

export function useDocAutosave(options: {
  workspaceId: () => string | null
  path: () => string | null
  sessionId: () => string | null
  /** The current buffer, for the full-buffer resend. */
  readText: () => string
  /** Replace the editor with text the server reconciled. */
  onRemoteText: (text: string) => void
}): {
  noteChange: (change: ChangeSet) => void
  flush: () => Promise<void>
  begin: (version: string | null) => void
  adopt: (content: string, version: string) => void
  dirty: () => boolean
  status: () => SaveStatus
} {
  const [status, setStatus] = createSignal<SaveStatus>({ kind: 'idle' })
  const [dirty, setDirty] = createSignal(false)

  let base: string | null = null
  let pending: ChangeSet | null = null
  let full = false
  let inflight = false
  let timer: number | null = null
  let attempt = 0

  function clearTimer() {
    if (timer !== null) {
      window.clearTimeout(timer)
      timer = null
    }
  }

  function schedule(delay = AUTOSAVE_DELAY_MS) {
    clearTimer()
    timer = window.setTimeout(() => {
      timer = null
      void flush()
    }, delay)
  }

  function markDirty(value: boolean) {
    setDirty(value)
  }

  /** The editor reports a user change. Accumulate it and arm the debounce. */
  function noteChange(change: ChangeSet) {
    pending = pending ? pending.compose(change) : change
    markDirty(true)
    schedule()
  }

  /** Start a new document: forget the pending set and adopt this base. */
  function begin(version: string | null) {
    clearTimer()
    base = version
    pending = null
    full = false
    attempt = 0
    markDirty(false)
    setStatus((current) => (current.kind === 'idle' ? current : { kind: 'idle' }))
  }

  /** Adopt text the server reconciled, when the buffer is clean. */
  function adopt(content: string, version: string) {
    options.onRemoteText(content)
    base = version
    pending = null
    full = false
    markDirty(false)
    setStatus((current) => (current.kind === 'idle' ? current : { kind: 'idle' }))
  }

  function backoff(): number {
    return Math.min(AUTOSAVE_DELAY_MS * 2 ** attempt, MAX_BACKOFF_MS)
  }

  async function send() {
    if (inflight) {
      return
    }
    if (!full && !pending) {
      return
    }
    const workspaceId = options.workspaceId()
    const path = options.path()
    if (!workspaceId || !path) {
      return
    }

    clearTimer()
    inflight = true
    setStatus({ kind: 'saving' })

    // Take the pending set. Changes that arrive during the flight re-accumulate.
    const delta = full ? null : pending
    const text = full ? options.readText() : null
    pending = null

    try {
      const result = await putDoc(workspaceId, path, {
        session_id: options.sessionId(),
        base_version: base,
        ...(text !== null ? { content: text } : { changes: serializeChangeSet(delta!) })
      })
      inflight = false

      if (result.kind === 'saved') {
        base = result.doc.version
        full = false
        attempt = 0
        // A merge may have folded in an agent edit; show the result.
        if (result.doc.outcome === 'merged' && result.doc.content !== options.readText()) {
          options.onRemoteText(result.doc.content)
        }
        if (pending) {
          markDirty(true)
          schedule()
        } else {
          markDirty(false)
          setStatus({ kind: 'saved', outcome: result.doc.outcome })
        }
        return
      }

      // The base version is gone. Re-send the whole buffer against the disk's
      // version; the user's text is the last edit, so it wins outright.
      base = result.version
      full = true
      pending = null
      markDirty(true)
      setStatus({ kind: 'out-of-sync' })
      schedule(0)
    } catch (err) {
      inflight = false
      // Restore what this attempt took, so nothing is lost.
      if (delta) {
        pending = pending ? delta.compose(pending) : delta
      }
      markDirty(true)
      setStatus({
        kind: 'error',
        message: err instanceof Error ? err.message : 'Failed to save the document'
      })
      // The first retry waits the debounce; each later one doubles.
      schedule(backoff())
      attempt += 1
    }
  }

  function flush(): Promise<void> {
    clearTimer()
    return send()
  }

  onCleanup(() => {
    clearTimer()
  })

  return { noteChange, flush, begin, adopt, dirty, status }
}
