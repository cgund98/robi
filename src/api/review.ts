import { api } from './client'
import { ApiError, errorMessage, statusOf } from './sessions'
import type { components } from './schema'

export type SessionReview = components['schemas']['SessionReview']
export type ReviewFile = components['schemas']['ReviewFileBody']
export type ReviewLine = components['schemas']['ReviewLineBody']

export type ReviewHunk = components['schemas']['ReviewHunkBody']

export async function decideReview(
  sessionId: string,
  path: string,
  decision: 'approve' | 'reject',
  hunkId?: string
): Promise<void> {
  const result = await api.POST('/api/v1/chat_sessions/{id}/review', {
    params: { path: { id: sessionId } },
    body: { path, decision, hunk_id: hunkId ?? null }
  })
  if (result.response.ok) {
    return
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to update review')
  )
}

export async function getSessionReview(sessionId: string): Promise<SessionReview> {
  const result = await api.GET('/api/v1/chat_sessions/{id}/review', {
    params: { path: { id: sessionId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load review')
  )
}
