import { apiBaseUrl } from '../../api/client'

/**
 * Agent event names and the stream URL.
 *
 * Types match `domain/events/envelope.rs`. The shell asks for every agent type
 * on one connection.
 */

export const AGENT_EVENT_TYPES = [
  'robi.agent.v1.turn_started',
  'robi.agent.v1.message_added',
  'robi.agent.v1.message_updated',
  'robi.agent.v1.message_delta',
  'robi.agent.v1.tool_call_updated',
  'robi.agent.v1.awaiting_approval',
  'robi.agent.v1.turn_finished',
  'robi.agent.v1.transcript_compacted',
  'robi.session.v1.created',
  'robi.session.v1.updated',
  'robi.session.v1.deleted',
  'robi.app.v1.error',
  'robi.index.v1.progress',
  'robi.mcp.v1.status'
] as const

export type AgentEventType = (typeof AGENT_EVENT_TYPES)[number]

export type EventEnvelope = {
  specversion: string
  id: string
  source: string
  type: string
  time: string
  subject: string
  data: unknown
}

export function buildAgentEventsStreamUrl(
  sessionId: string | null,
  baseUrl?: string,
  workspaceId?: string | null
): string {
  const origin = baseUrl ?? apiBaseUrl()
  const params = new URLSearchParams()
  if (sessionId) {
    params.set('session_id', sessionId)
  } else if (workspaceId) {
    params.set('workspace_id', workspaceId)
  }
  for (const eventType of AGENT_EVENT_TYPES) {
    params.append('event_types', eventType)
  }
  return `${origin}/api/v1/events/stream?${params.toString()}`
}

/** `null` when the frame is not a CloudEvents object. The stream stays open. */
export function parseEventEnvelope(raw: string): EventEnvelope | null {
  try {
    const value: unknown = JSON.parse(raw)
    if (value == null || typeof value !== 'object') {
      return null
    }
    const record = value as Record<string, unknown>
    if (
      typeof record.specversion !== 'string' ||
      typeof record.id !== 'string' ||
      typeof record.source !== 'string' ||
      typeof record.type !== 'string' ||
      typeof record.time !== 'string' ||
      typeof record.subject !== 'string' ||
      !('data' in record)
    ) {
      return null
    }
    return {
      specversion: record.specversion,
      id: record.id,
      source: record.source,
      type: record.type,
      time: record.time,
      subject: record.subject,
      data: record.data
    }
  } catch {
    return null
  }
}
