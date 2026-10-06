/** Unsent composer text, keyed by session id or `draft` for a chat with no row. */
const drafts = new Map<string, string>()
const listeners = new Set<() => void>()

export function subscribeComposerDrafts(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function readComposerDraft(key: string): string {
  return drafts.get(key) ?? ''
}

export function writeComposerDraft(key: string, value: string): void {
  if (value.length === 0) {
    if (!drafts.has(key)) {
      return
    }
    drafts.delete(key)
  } else if (drafts.get(key) === value) {
    return
  } else {
    drafts.set(key, value)
  }
  for (const listener of listeners) {
    listener()
  }
}

/**
 * Remove a draft after a successful send. A draft the user typed while the
 * send was in flight stays; only the text that was sent is dropped.
 */
export function dropSentComposerDraft(key: string, sent: string): void {
  const current = drafts.get(key)
  if (current === undefined || current === sent) {
    writeComposerDraft(key, '')
  }
}
