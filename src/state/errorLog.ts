import { create } from 'zustand'

/** One error the shell has seen since this page load. Not written to disk. */
export type ErrorEntry = {
  id: string
  message: string
  /** Chat session this error belongs to, or null for the app as a whole. */
  sessionId: string | null
  at: number
  /** Still showing in the shell. Acknowledged entries stay on the audit log. */
  open: boolean
}

type ErrorLogState = {
  entries: ErrorEntry[]
  report: (message: string, sessionId?: string | null) => void
  acknowledge: (id: string) => void
}

let nextId = 0

export const useErrorLog = create<ErrorLogState>((set) => ({
  entries: [],
  report: (message, sessionId = null) => {
    const entry: ErrorEntry = {
      id: `error-${++nextId}`,
      message,
      sessionId: sessionId ?? null,
      at: Date.now(),
      open: true
    }
    set((state) => ({ entries: [entry, ...state.entries] }))
  },
  acknowledge: (id) => {
    set((state) => ({
      entries: state.entries.map((entry) => (entry.id === id ? { ...entry, open: false } : entry))
    }))
  }
}))

/** Where an open error is drawn. Hidden entries stay on the audit log only. */
export function noticePlacement(
  entry: Pick<ErrorEntry, 'open' | 'sessionId'>,
  activeSessionId: string | null,
  transcriptVisible: boolean
): 'transcript' | 'top' | 'hidden' {
  if (!entry.open) {
    return 'hidden'
  }
  if (
    transcriptVisible &&
    entry.sessionId !== null &&
    activeSessionId !== null &&
    entry.sessionId === activeSessionId
  ) {
    return 'transcript'
  }
  return 'top'
}
