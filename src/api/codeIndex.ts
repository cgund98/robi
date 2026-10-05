import { api } from './client'
import type { components } from './schema'
import { ApiError, errorMessage, statusOf } from './sessions'

export type IndexStatus = components['schemas']['IndexStatusBody']

/** How long a successful index GET is reused before the next network call. */
export const INDEX_STATUS_TTL_MS = 10_000

const INDEX_STATUS_CACHE_LIMIT = 8

type CacheEntry = { status: IndexStatus; at: number }

const cache = new Map<string, CacheEntry>()
const inflight = new Map<string, Promise<IndexStatus>>()

function remember(id: string, status: IndexStatus, at = Date.now()): void {
  cache.delete(id)
  cache.set(id, { status, at })
  while (cache.size > INDEX_STATUS_CACHE_LIMIT) {
    const oldest = cache.keys().next().value
    if (oldest === undefined) {
      break
    }
    cache.delete(oldest)
  }
}

export function cachedIndexStatus(id: string, now = Date.now()): IndexStatus | null {
  const entry = cache.get(id)
  if (!entry || now - entry.at >= INDEX_STATUS_TTL_MS) {
    return null
  }
  remember(id, entry.status, entry.at)
  return entry.status
}

/** Test hook. */
export function clearIndexStatusCache(): void {
  cache.clear()
  inflight.clear()
}

export async function getIndexStatus(id: string): Promise<IndexStatus> {
  const hit = cachedIndexStatus(id)
  if (hit) {
    return hit
  }
  const pending = inflight.get(id)
  if (pending) {
    return pending
  }
  const request = fetchIndexStatus(id)
    .then((status) => {
      remember(id, status)
      return status
    })
    .finally(() => {
      inflight.delete(id)
    })
  inflight.set(id, request)
  return request
}

async function fetchIndexStatus(id: string): Promise<IndexStatus> {
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
    remember(id, result.data)
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to update the index')
  )
}
