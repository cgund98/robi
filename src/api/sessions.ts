import { api } from './client'
import type { components } from './schema'

export type ChatSession = components['schemas']['ChatSession']

export class ApiError extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

export function errorMessage(error: unknown, fallback: string): string {
  if (error && typeof error === 'object' && 'error' in error) {
    const value = (error as { error?: unknown }).error
    if (typeof value === 'string' && value.length > 0) {
      return value
    }
  }
  if (typeof error === 'string' && error.length > 0) {
    return error
  }
  return fallback
}

export function statusOf(response: { status: number } | undefined): number {
  return response?.status ?? 500
}

export async function listSessions(workspaceId: string): Promise<ChatSession[]> {
  const result = await api.GET('/api/v1/chat_sessions', {
    params: { query: { workspace_id: workspaceId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to list chat sessions')
  )
}

export async function createSession(
  workspaceId: string,
  title?: string | null
): Promise<ChatSession> {
  const result = await api.POST('/api/v1/chat_sessions', {
    body: {
      workspace_id: workspaceId,
      ...(title === undefined ? {} : { title })
    }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to create chat session')
  )
}

export async function getSession(id: string): Promise<ChatSession> {
  const result = await api.GET('/api/v1/chat_sessions/{id}', {
    params: { path: { id } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load chat session')
  )
}

export async function updateSession(id: string, title: string): Promise<ChatSession> {
  const result = await api.PATCH('/api/v1/chat_sessions/{id}', {
    params: { path: { id } },
    body: { title }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to rename chat session')
  )
}

export async function deleteSession(id: string): Promise<void> {
  const result = await api.DELETE('/api/v1/chat_sessions/{id}', {
    params: { path: { id } }
  })
  if (result.response.ok) {
    return
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to delete chat session')
  )
}

export function sessionDisplayTitle(
  session: Pick<ChatSession, 'title'> | null | undefined
): string {
  const title = session?.title?.trim()
  return title && title.length > 0 ? title : 'New chat'
}
