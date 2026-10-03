import { create } from 'zustand'

import { getIndexStatus, setIndexState, type IndexStatus } from '../api/codeIndex'
import { fetchStillCurrent, startFetch } from '../app/latestFetch'
import { useWorkspaceStore } from './workspaceStore'

/** Pause or resume has been requested and the task has not caught up. */
export type IndexPending = 'pause' | 'resume'

type IndexStore = {
  workspaceId: string | null
  status: IndexStatus | null
  pending: IndexPending | null
  apply: (workspaceId: string, status: IndexStatus) => void
  refresh: (workspaceId: string) => Promise<void>
  setPaused: (paused: boolean) => Promise<void>
}

export const useIndexStore = create<IndexStore>((set, get) => ({
  workspaceId: null,
  status: null,
  pending: null,
  apply: (workspaceId, status) => {
    if (workspaceId !== useWorkspaceStore.getState().activeWorkspaceId) {
      return
    }
    set((state) => ({
      workspaceId,
      status,
      pending: clearPending(state.pending, status.state)
    }))
  },
  refresh: async (workspaceId) => {
    if (workspaceId !== useWorkspaceStore.getState().activeWorkspaceId) {
      return
    }
    const key = `index:${workspaceId}`
    const generation = startFetch(key)
    try {
      const status = await getIndexStatus(workspaceId)
      if (
        workspaceId === useWorkspaceStore.getState().activeWorkspaceId &&
        fetchStillCurrent(key, generation)
      ) {
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
  setPaused: async (paused) => {
    const workspaceId = get().workspaceId ?? useWorkspaceStore.getState().activeWorkspaceId
    if (!workspaceId || get().pending) {
      return
    }
    const pending: IndexPending = paused ? 'pause' : 'resume'
    set({ pending })
    try {
      const status = await setIndexState(workspaceId, paused ? 'paused' : 'running')
      if (workspaceId !== useWorkspaceStore.getState().activeWorkspaceId) {
        return
      }
      set((state) => ({
        workspaceId,
        status,
        pending: state.pending === pending ? clearPending(pending, status.state) : state.pending
      }))
    } catch {
      if (workspaceId === useWorkspaceStore.getState().activeWorkspaceId) {
        set({ pending: null })
      }
    }
  }
}))

function clearPending(pending: IndexPending | null, state: string): IndexPending | null {
  if (!pending) {
    return null
  }
  if (pending === 'pause') {
    return state === 'indexing' || state === 'downloading' ? pending : null
  }
  return state === 'paused' ? pending : null
}
