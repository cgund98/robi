import { api } from './client'
import type { components } from './schema'
import { ApiError, errorMessage, statusOf } from './sessions'

export type DocsListing = components['schemas']['DocsListing']
export type DocEntry = components['schemas']['DocEntry']
export type DocContent = components['schemas']['DocContent']
export type DocSearchResult = components['schemas']['DocSearchResult']
export type DocSearchHit = components['schemas']['DocSearchHit']
export type DocSearchEngine = 'semantic' | 'ripgrep'

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

/**
 * Search the workspace's markdown. `semantic` is the index; `ripgrep` is a
 * literal scan and does not start the index. The response names the engine.
 * For `semantic`, anything but `ready` means the results are incomplete.
 */
export async function searchDocs(
  workspaceId: string,
  q: string,
  limit?: number,
  signal?: AbortSignal,
  engine?: DocSearchEngine
): Promise<DocSearchResult> {
  const result = await api.GET('/api/v1/workspaces/{id}/docs/search', {
    params: {
      path: { id: workspaceId },
      query: {
        q,
        ...(limit === undefined ? {} : { limit }),
        ...(engine === undefined ? {} : { engine })
      }
    },
    signal
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to search docs')
  )
}
