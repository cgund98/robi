import { create } from 'zustand'

import type { StoreSet } from './storeDeps'

/** One API call from this page load. Not written to disk. */
export type RequestEntry = {
  id: string
  method: string
  /** OpenAPI path, with `{param}` placeholders. */
  path: string
  /** HTTP status, or null when the fetch threw before a response. */
  status: number | null
  durationMs: number
  /** Response body, or the error text when the fetch threw. */
  body: string
  at: number
}

const MAX_ENTRIES = 250

export type RequestLogState = {
  entries: RequestEntry[]
  record: (entry: Omit<RequestEntry, 'id' | 'at'>) => void
}

let nextId = 0

export function createRequestLog(set: StoreSet<RequestLogState>): RequestLogState {
  return {
    entries: [],
    record: (entry) => {
      const row: RequestEntry = {
        id: `request-${++nextId}`,
        at: Date.now(),
        ...entry
      }
      set((state) => ({ entries: [row, ...state.entries].slice(0, MAX_ENTRIES) }))
    }
  }
}

export const useRequestLog = create<RequestLogState>((set) =>
  createRequestLog((partial) => set(partial))
)
