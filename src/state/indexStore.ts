import { mountStore } from './storeDeps'

import { getIndexStatus, setIndexState, type IndexStatus } from '../api/codeIndex'
import { fetchStillCurrent, startFetch } from '../app/latestFetch'
import type { ActiveWorkspace, StoreGet, StoreSet } from './storeDeps'
import { workspaces } from './workspaceStore'

/** Pause or resume has been requested and the task has not caught up. */
export type IndexPending = 'pause' | 'resume'

export type IndexStore = {
  workspaceId: string | null
  status: IndexStatus | null
  pending: IndexPending | null
  refresh: (workspaceId: string) => Promise<void>
  applyFrame: (workspaceId: string, data: unknown) => boolean
  setPaused: (paused: boolean) => Promise<void>
}

export function createIndexState(
  set: StoreSet<IndexStore>,
  get: StoreGet<IndexStore>,
  deps: ActiveWorkspace
): IndexStore {
  return {
    workspaceId: null,
    status: null,
    pending: null,
    refresh: async (workspaceId) => {
      if (workspaceId !== deps.activeWorkspaceId()) {
        return
      }
      const key = `index:${workspaceId}`
      const generation = startFetch(key)
      try {
        const status = await getIndexStatus(workspaceId)
        if (workspaceId === deps.activeWorkspaceId() && fetchStillCurrent(key, generation)) {
          set((state) => ({
            workspaceId,
            status,
            pending: clearPending(state.pending, status.state)
          }))
        }
      } catch {
        if (fetchStillCurrent(key, generation)) {
          set({ workspaceId, status: null, pending: null })
        }
      }
    },
    applyFrame: (workspaceId, data) => {
      if (workspaceId !== deps.activeWorkspaceId()) {
        return false
      }
      const status = toStatus(data)
      if (!status) {
        return false
      }
      set((state) => ({
        workspaceId,
        status,
        pending: clearPending(state.pending, status.state)
      }))
      return true
    },
    setPaused: async (paused) => {
      const workspaceId = get().workspaceId ?? deps.activeWorkspaceId()
      if (!workspaceId || get().pending) {
        return
      }
      const pending: IndexPending = paused ? 'pause' : 'resume'
      set({ pending })
      try {
        const status = await setIndexState(workspaceId, paused ? 'paused' : 'running')
        if (workspaceId !== deps.activeWorkspaceId()) {
          return
        }
        set((state) => ({
          workspaceId,
          status,
          pending: state.pending === pending ? clearPending(pending, status.state) : state.pending
        }))
      } catch {
        if (workspaceId === deps.activeWorkspaceId()) {
          set({ pending: null })
        }
      }
    }
  }
}

const indexHost = mountStore<IndexStore>((set, get) =>
  createIndexState(set, get, {
    activeWorkspaceId: () => workspaces.activeWorkspaceId
  })
)

export const index = indexHost.state

export function patchIndex(partial: Partial<IndexStore>): void {
  indexHost.set(partial)
}

function clearPending(pending: IndexPending | null, state: string): IndexPending | null {
  if (!pending) {
    return null
  }
  if (pending === 'pause') {
    return state === 'indexing' || state === 'downloading' ? pending : null
  }
  return state === 'paused' ? pending : null
}

/** The frame `data` is the same object as the GET body. Reject anything else. */
function toStatus(data: unknown): IndexStatus | null {
  if (!data || typeof data !== 'object') {
    return null
  }
  const { state, files_done, files_total, error } = data as Record<string, unknown>
  if (
    typeof state !== 'string' ||
    typeof files_done !== 'number' ||
    typeof files_total !== 'number'
  ) {
    return null
  }
  return {
    state,
    files_done,
    files_total,
    error: typeof error === 'string' ? error : null
  }
}
