import { api } from './client'
import type { components } from './schema'
import { ApiError, errorMessage, statusOf } from './sessions'

export type IndexStatus = components['schemas']['IndexStatusBody']

export async function getIndexStatus(id: string): Promise<IndexStatus> {
  const result = await api.GET('/api/v1/workspaces/{id}/index', {
    params: { path: { id } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load the index')
  )
}

export async function setIndexState(id: string, state: 'paused' | 'running'): Promise<IndexStatus> {
  const result = await api.PUT('/api/v1/workspaces/{id}/index', {
    params: { path: { id } },
    body: { state }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to update the index')
  )
}
