import { create } from 'zustand'

import type { StoreSet } from './storeDeps'

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

export type ErrorLogState = {
  entries: ErrorEntry[]
  report: (message: string, sessionId?: string | null) => void
  /** Audit log only. Does not open a shell notice. */
  record: (message: string) => void
  acknowledge: (id: string) => void
}

let nextId = 0

/** A fetch this page cancelled. It is not a failure to keep. */
function isAbortText(message: string): boolean {
  return (
    message === 'Fetch is aborted' ||
    message === 'The operation was aborted.' ||
    message === 'The user aborted a request.'
  )
}

export function createErrorLog(set: StoreSet<ErrorLogState>): ErrorLogState {
  return {
    entries: [],
    report: (message, sessionId = null) => {
      if (isAbortText(message)) {
        return
      }
      const entry: ErrorEntry = {
        id: `error-${++nextId}`,
        message,
        sessionId: sessionId ?? null,
        at: Date.now(),
        open: true
      }
      set((state) => ({ entries: [entry, ...state.entries] }))
    },
    record: (message) => {
      if (isAbortText(message) || message.endsWith('(Fetch is aborted)')) {
        return
      }
      const entry: ErrorEntry = {
        id: `error-${++nextId}`,
        message,
        sessionId: null,
        at: Date.now(),
        open: false
      }
      set((state) => ({ entries: [entry, ...state.entries] }))
    },
    acknowledge: (id) => {
      set((state) => ({
        entries: state.entries.map((entry) => (entry.id === id ? { ...entry, open: false } : entry))
      }))
    }
  }
}

export const useErrorLog = create<ErrorLogState>((set) => createErrorLog((partial) => set(partial)))

/**
 * Where an open error is drawn. Every open error sits under the window title,
 * including one for the chat on screen. The bar above the composer grows
 * upward and can push that notice off the top of the window. Hidden entries
 * stay on the audit log only.
 */
export function noticePlacement(entry: Pick<ErrorEntry, 'open'>): 'top' | 'hidden' {
  if (!entry.open) {
    return 'hidden'
  }
  return 'top'
}
