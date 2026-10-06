import type { FileAttachment } from '../components/chat/textAttachments'

/**
 * One attachment waiting for a composer to mount, keyed by the composer's
 * `draftKey` (a session id, or `draft` for a chat with no row yet).
 *
 * The docs viewer can attach a line while the chat tray is closed, and the tray
 * unmounts its composer while closed — so the request has to outlive the
 * component that would receive it. A keyed one-shot queue does that: the request
 * waits until the matching composer drains it on mount.
 */
const pending = new Map<string, FileAttachment[]>()
const listeners = new Set<() => void>()

export function subscribeComposerAttachments(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/** Queue one attachment for the composer keyed `key`. */
export function requestComposerAttachment(key: string, file: FileAttachment): void {
  const queue = pending.get(key)
  if (queue) {
    queue.push(file)
  } else {
    pending.set(key, [file])
  }
  for (const listener of listeners) {
    listener()
  }
}

/** Remove and return every attachment queued for `key`. */
export function takeComposerAttachments(key: string): FileAttachment[] {
  const queue = pending.get(key)
  if (!queue) {
    return []
  }
  pending.delete(key)
  return queue
}
