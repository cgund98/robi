import { api } from './client'
import type { components } from './schema'
import { ApiError, errorMessage, statusOf } from './sessions'

export type DocsListing = components['schemas']['DocsListing']
export type DocEntry = components['schemas']['DocEntry']
export type DocContent = components['schemas']['DocContent']
export type DocWriteRequest = components['schemas']['DocWriteRequest']
export type DocWriteResponse = components['schemas']['DocWriteResponse']
export type DocWriteConflict = components['schemas']['DocWriteConflict']
export type DocSearchResult = components['schemas']['DocSearchResult']
export type DocSearchHit = components['schemas']['DocSearchHit']
export type DocSearchEngine = 'semantic' | 'ripgrep'

export async function listDocs(
  workspaceId: string,
  options?: { path?: string; recursive?: boolean }
): Promise<DocsListing> {
  const result = await api.GET('/api/v1/workspaces/{id}/docs', {
    params: {
      path: { id: workspaceId },
      query: {
        path: options?.path,
        recursive: options?.recursive
      }
    }
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
 * The result of a save. `saved` carries the reconciled text and its new
 * version. `conflict` means the base version is gone: re-send the whole buffer
 * against `version`.
 */
export type DocSaveResult =
  { kind: 'saved'; doc: DocWriteResponse } | { kind: 'conflict'; content: string; version: string }

/**
 * Save the open document. The body is either a CodeMirror change set
 * (`baseVersion` + `changes`) or the whole buffer (`content`).
 */
export async function putDoc(
  workspaceId: string,
  path: string,
  body: DocWriteRequest
): Promise<DocSaveResult> {
  const result = await api.PUT('/api/v1/workspaces/{id}/docs/{path}', {
    params: { path: { id: workspaceId, path } },
    body
  })
  if (result.data) {
    return { kind: 'saved', doc: result.data }
  }
  if (result.response.status === 409) {
    const conflict = result.error as DocWriteConflict | undefined
    if (conflict && typeof conflict.content === 'string') {
      return { kind: 'conflict', content: conflict.content, version: conflict.version }
    }
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to save document')
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
