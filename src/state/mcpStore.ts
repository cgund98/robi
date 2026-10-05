import { create } from 'zustand'

import { listMcpServers, type McpServer } from '../api/mcp'
import { fetchStillCurrent, startFetch } from '../app/latestFetch'
import { useWorkspaceStore } from './workspaceStore'

type McpState = {
  workspaceId: string | null
  servers: McpServer[]
  refresh: (workspaceId: string) => Promise<void>
}

export const useMcpStore = create<McpState>((set) => ({
  workspaceId: null,
  servers: [],
  refresh: async (workspaceId) => {
    if (workspaceId !== useWorkspaceStore.getState().activeWorkspaceId) {
      return
    }
    const key = `mcp:${workspaceId}`
    const generation = startFetch(key)
    try {
      const servers = await listMcpServers(workspaceId)
      if (
        workspaceId === useWorkspaceStore.getState().activeWorkspaceId &&
        fetchStillCurrent(key, generation)
      ) {
        set({ workspaceId, servers })
      }
    } catch {
      if (fetchStillCurrent(key, generation)) {
        set({ workspaceId, servers: [] })
      }
    }
  }
}))
