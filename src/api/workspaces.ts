import { api } from './client'
import type { components } from './schema'
import { ApiError, errorMessage, statusOf } from './sessions'

export type Workspace = components['schemas']['Workspace']

export async function listWorkspaces(): Promise<Workspace[]> {
  const result = await api.GET('/api/v1/workspaces')
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to list workspaces')
  )
}

export async function createWorkspace(root: string): Promise<Workspace> {
  const result = await api.POST('/api/v1/workspaces', {
    body: { root }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to add workspace')
  )
}

export async function deleteWorkspace(id: string): Promise<void> {
  const result = await api.DELETE('/api/v1/workspaces/{id}', {
    params: { path: { id } }
  })
  if (result.response.ok) {
    return
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to remove workspace')
  )
}
