import { listMcpServers, type McpServer } from '../api/mcp'
import { fetchStillCurrent, startFetch } from '../app/latestFetch'
import { mountStore, type ActiveWorkspace, type StoreSet } from './storeDeps'
import { workspaces } from './workspaceStore'

export type McpState = {
  workspaceId: string | null
  servers: McpServer[]
  refresh: (workspaceId: string) => Promise<void>
}

export function createMcpState(set: StoreSet<McpState>, deps: ActiveWorkspace): McpState {
  return {
    workspaceId: null,
    servers: [],
    refresh: async (workspaceId) => {
      if (workspaceId !== deps.activeWorkspaceId()) {
        return
      }
      const key = `mcp:${workspaceId}`
      const generation = startFetch(key)
      try {
        const servers = await listMcpServers(workspaceId)
        if (workspaceId === deps.activeWorkspaceId() && fetchStillCurrent(key, generation)) {
          set({ workspaceId, servers })
        }
      } catch {
        if (fetchStillCurrent(key, generation)) {
          set({ workspaceId, servers: [] })
        }
      }
    }
  }
}

const mcpHost = mountStore<McpState>((set) =>
  createMcpState(set, {
    activeWorkspaceId: () => workspaces.activeWorkspaceId
  })
)

export const mcp = mcpHost.state

export function patchMcp(partial: Partial<McpState>): void {
  mcpHost.set(partial)
}
