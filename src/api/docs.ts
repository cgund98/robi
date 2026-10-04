import { api } from './client'
import type { components } from './schema'
import { ApiError, errorMessage, statusOf } from './sessions'

export type DocsListing = components['schemas']['DocsListing']
export type DocEntry = components['schemas']['DocEntry']
export type DocContent = components['schemas']['DocContent']

export async function listDocs(workspaceId: string): Promise<DocsListing> {
  const result = await api.GET('/api/v1/workspaces/{id}/docs', {
    params: { path: { id: workspaceId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load docs')
  )
}

export async function getDoc(workspaceId: string, path: string): Promise<DocContent> {
  const result = await api.GET('/api/v1/workspaces/{id}/docs/{path}', {
    params: {
      path: { id: workspaceId, path }
    }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load document')
  )
}
