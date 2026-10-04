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

/** Move a new-chat draft onto the session row created by its first send. */
export function claimComposerDraft(sessionId: string, text: string): void {
  writeComposerDraft('draft', '')
  writeComposerDraft(sessionId, text)
}
