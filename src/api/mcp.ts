import { ApiError, errorMessage, statusOf } from './sessions'
import { api } from './client'
import type { components } from './schema'

export type McpServer = components['schemas']['McpServer']
export type McpConfig = components['schemas']['McpConfig']

export async function listMcpServers(workspaceId: string): Promise<McpServer[]> {
  const result = await api.GET('/api/v1/workspaces/{id}/mcp', {
    params: { path: { id: workspaceId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load MCP servers')
  )
}

export async function getMcpConfig(workspaceId: string): Promise<McpConfig> {
  const result = await api.GET('/api/v1/workspaces/{id}/mcp/config', {
    params: { path: { id: workspaceId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load MCP config')
  )
}
