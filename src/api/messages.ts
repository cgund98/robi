import { ApiError, errorMessage, statusOf } from './sessions'
import { api, apiBaseUrl, fetchWithTimeout } from './client'
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

export type ToolOriginal = components['schemas']['ToolOriginal']

export async function getToolOriginal(
  sessionId: string,
  originalId: string
): Promise<ToolOriginal> {
  const result = await api.GET('/api/v1/chat_sessions/{id}/tool_originals/{original_id}', {
    params: { path: { id: sessionId, original_id: originalId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load command output')
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

export async function stopSession(sessionId: string): Promise<void> {
  const result = await api.POST('/api/v1/chat_sessions/{id}/stop', {
    params: { path: { id: sessionId } }
  })
  if (result.response.ok) {
    return
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to stop')
  )
}

export async function submitInstruction(
  sessionId: string,
  instruction: string,
  images?: File[]
): Promise<void> {
  const hasImages = (images?.length ?? 0) > 0
  let response: Response
  if (hasImages) {
    // A message with images goes out as `multipart/form-data`. openapi-fetch's
    // typed client re-reads the body as text, which would strip the multipart
    // boundary, so the upload uses our fetch path with a real FormData body.
    const form = new FormData()
    form.append('instruction', instruction)
    for (const image of images!) {
      form.append('images', image, image.name)
    }
    response = await fetchWithTimeout(
      `${apiBaseUrl()}/api/v1/chat_sessions/${sessionId}/messages`,
      {
        method: 'POST',
        body: form
      }
    )
  } else {
    const result = await api.POST('/api/v1/chat_sessions/{id}/messages', {
      params: { path: { id: sessionId } },
      body: { instruction }
    })
    response = result.response
  }
  if (response.ok) {
    return
  }
  throw new ApiError(
    statusOf(response as { status: number } | undefined),
    errorMessage(await safeError(response), 'Failed to send message')
  )
}

async function safeError(response: Response): Promise<string | Error | undefined> {
  try {
    const body = await response.json()
    return (body as { error?: string })?.error ?? 'Failed to send message'
  } catch {
    return 'Failed to send message'
  }
}

/** The served URL for a stored image, for a message bubble's `src`.
 *
 * The desktop page is not the API host. A path-only URL would load from the
 * webview origin and the thumbnail would fail. Prefix the bound API origin
 * the same way other requests do.
 */
export function imageUrl(sessionId: string, imageId: string): string {
  const path = `/api/v1/chat_sessions/${sessionId}/images/${imageId}`
  const base = apiBaseUrl().replace(/\/$/, '')
  return base ? `${base}${path}` : path
}
