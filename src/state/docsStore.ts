import { mountStore, type StoreSet } from './storeDeps'

/**
 * The most recent `robi.workspace.v1.file_changed` frame.
 *
 * `seq` increments on every frame so a viewer memo re-runs even when two
 * frames name the same path and outcome.
 */
export type DocsFileChange = {
  seq: number
  path: string
  source: 'user' | 'agent'
  outcome: string
  sessionId: string | null
}

export type DocsStore = {
  change: DocsFileChange | null
  record: (change: Omit<DocsFileChange, 'seq'>) => void
}

export function createDocsState(set: StoreSet<DocsStore>, get: () => DocsStore): DocsStore {
  return {
    change: null,
    record: (change) => {
      const seq = (get().change?.seq ?? 0) + 1
      set({ change: { seq, ...change } })
    }
  }
}

const docsHost = mountStore<DocsStore>((set, get) => createDocsState(set, get))

export const docs = docsHost.state

export function patchDocs(partial: Partial<DocsStore>): void {
  docsHost.set(partial)
}
