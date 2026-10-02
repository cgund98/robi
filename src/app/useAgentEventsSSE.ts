import { useCallback } from 'react'

import { getMessage } from '../api/messages'
import { ApiError } from '../api/sessions'
import { useReconnectingEventSource } from '../infra/useReconnectingEventSource'
import { useChatStore } from '../state/chatStore'
import { AGENT_EVENT_TYPES, buildAgentEventsStreamUrl, parseEventEnvelope } from './agentEvents'

type DeltaData = {
  session_id?: string
  message_id?: string
  delta?: { kind?: string }
  outcome?: { kind?: string; message?: string }
}

function eventData(data: unknown): DeltaData | null {
  if (!data || typeof data !== 'object') {
    return null
  }
  return data as DeltaData
}

/**
 * One session-scoped event stream for the shell. Frames refetch HTTP; they
 * are not written into the transcript. No connection while a draft is open.
 */
export function useAgentEventsSSE(): void {
  const activeSessionId = useChatStore((state) => state.activeSessionId)
  const draftSelected = useChatStore((state) => state.draftSelected)
  const url = !draftSelected && activeSessionId ? buildAgentEventsStreamUrl(activeSessionId) : null

  const onEvent = useCallback((event: MessageEvent) => {
    if (typeof event.data !== 'string') {
      return
    }
    const envelope = parseEventEnvelope(event.data)
    if (!envelope) {
      return
    }
    const data = eventData(envelope.data)
    const sessionId = envelope.subject || data?.session_id
    if (!sessionId) {
      return
    }

    switch (envelope.type) {
      case 'robi.agent.v1.turn_started':
        useChatStore.getState().setPhase(sessionId, 'thinking')
        return
      case 'robi.agent.v1.message_added':
      case 'robi.agent.v1.message_updated': {
        const messageId = data?.message_id
        if (!messageId) {
          return
        }
        void getMessage(sessionId, messageId)
          .then((message) => {
            useChatStore.getState().upsertMessage(sessionId, message)
          })
          .catch((err: unknown) => {
            if (err instanceof ApiError && err.status === 404) {
              return
            }
            const message = err instanceof Error ? err.message : 'Failed to load message'
            useChatStore.setState({ error: message })
          })
        return
      }
      case 'robi.agent.v1.message_delta': {
        const kind = data?.delta?.kind
        if (kind) {
          useChatStore.getState().noteDelta(sessionId, kind)
        }
        return
      }
      case 'robi.agent.v1.turn_finished': {
        const outcome = data?.outcome
        const failed = outcome?.kind === 'failed' ? (outcome.message ?? 'The turn failed') : null
        void useChatStore.getState().finishTurn(sessionId, failed)
        return
      }
      case 'robi.agent.v1.session_updated':
        void useChatStore.getState().refreshSession(sessionId)
        return
      default:
        return
    }
  }, [])

  const onOpen = useCallback(() => {
    void useChatStore.getState().hydrateFromStream()
  }, [])

  useReconnectingEventSource({
    url,
    eventTypes: AGENT_EVENT_TYPES,
    onEvent,
    onOpen
  })
}
