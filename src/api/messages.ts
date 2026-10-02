import { ApiError, errorMessage, statusOf } from './sessions'
import { api } from './client'
import type { components } from './schema'

export type ChatMessage = components['schemas']['ChatMessage']

export async function listMessages(sessionId: string): Promise<ChatMessage[]> {
  const result = await api.GET('/api/v1/chat_sessions/{id}/messages', {
    params: { path: { id: sessionId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load messages')
  )
}

export async function getMessage(sessionId: string, messageId: string): Promise<ChatMessage> {
  const result = await api.GET('/api/v1/chat_sessions/{id}/messages/{message_id}', {
    params: { path: { id: sessionId, message_id: messageId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load message')
  )
}

export async function decideToolCall(
  sessionId: string,
  callId: string,
  decision: 'approve' | 'reject'
): Promise<void> {
  const result = await api.POST('/api/v1/chat_sessions/{id}/tool_calls/{call_id}', {
    params: { path: { id: sessionId, call_id: callId } },
    body: { decision }
  })
  if (result.response.ok) {
    return
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to settle the tool call')
  )
}

export async function submitInstruction(sessionId: string, instruction: string): Promise<void> {
  const result = await api.POST('/api/v1/chat_sessions/{id}/messages', {
    params: { path: { id: sessionId } },
    body: { instruction }
  })
  if (result.response.ok) {
    return
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to send message')
  )
}
