import { useCallback, useEffect, useRef } from 'react'

import { getMessage } from '../api/messages'
import { ApiError } from '../api/sessions'
import { useReconnectingEventSource } from '../infra/useReconnectingEventSource'
import { useChatStore } from '../state/chatStore'
import { useIndexStore } from '../state/indexStore'
import { postApprovalNotice, releaseApprovalPause } from './approvalNotice'
import { AGENT_EVENT_TYPES, buildAgentEventsStreamUrl, parseEventEnvelope } from './agentEvents'
import { fetchStillCurrent, startFetch } from './latestFetch'
import { transcriptCatchUpDue } from './transcriptCatchUp'

type DeltaData = {
  session_id?: string
  message_id?: string
  tool_call_id?: string
  message?: string
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
  const phase = useChatStore((state) =>
    activeSessionId ? state.phaseBySession[activeSessionId] : undefined
  )
  const url = !draftSelected && activeSessionId ? buildAgentEventsStreamUrl(activeSessionId) : null
  const heardAt = useRef(0)

  const onEvent = useCallback((event: MessageEvent) => {
    if (typeof event.data !== 'string') {
      return
    }
    const envelope = parseEventEnvelope(event.data)
    if (!envelope) {
      return
    }
    if (envelope.type === 'robi.index.v1.progress') {
      if (envelope.subject) {
        void useIndexStore.getState().refresh(envelope.subject)
      }
      return
    }
    heardAt.current = Date.now()
    const data = eventData(envelope.data)
    const sessionId = envelope.subject || data?.session_id
    if (!sessionId) {
      return
    }

    switch (envelope.type) {
      case 'robi.agent.v1.turn_started':
        releaseApprovalPause(sessionId)
        useChatStore.getState().setPhase(sessionId, 'thinking')
        return
      case 'robi.agent.v1.awaiting_approval':
        void postApprovalNotice(sessionId, data?.tool_call_id)
        return
      case 'robi.agent.v1.message_added':
      case 'robi.agent.v1.message_updated':
      case 'robi.agent.v1.tool_call_updated': {
        if (envelope.type === 'robi.agent.v1.tool_call_updated') {
          useChatStore.getState().bumpReview(sessionId)
        }
        const messageId = data?.message_id
        if (!messageId) {
          return
        }
        const key = `message:${sessionId}:${messageId}`
        const generation = startFetch(key)
        void getMessage(sessionId, messageId)
          .then((message) => {
            if (!fetchStillCurrent(key, generation)) {
              return
            }
            useChatStore.getState().upsertMessage(sessionId, message)
          })
          .catch((err: unknown) => {
            if (!fetchStillCurrent(key, generation)) {
              return
            }
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
        useChatStore.getState().bumpReview(sessionId)
        const outcome = data?.outcome
        const failed = outcome?.kind === 'failed' ? (outcome.message ?? 'The turn failed') : null
        void useChatStore.getState().finishTurn(sessionId, failed)
        return
      }
      case 'robi.session.v1.created':
      case 'robi.session.v1.updated': {
        const id = typeof data?.session_id === 'string' ? data.session_id : sessionId
        void useChatStore.getState().refreshSession(id)
        return
      }
      case 'robi.session.v1.deleted': {
        const id = typeof data?.session_id === 'string' ? data.session_id : null
        if (id) {
          void useChatStore.getState().forgetSession(id)
        }
        return
      }
      case 'robi.app.v1.error': {
        const message = data?.message
        if (typeof message === 'string' && message.length > 0) {
          useChatStore.setState({ error: message })
        }
        return
      }
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

  useEffect(() => {
    heardAt.current = Date.now()
    if (!activeSessionId || draftSelected) {
      return
    }
    if (phase !== 'thinking' && phase !== 'responding') {
      return
    }
    const timer = window.setInterval(() => {
      const current = useChatStore.getState().phaseBySession[activeSessionId]
      if (!transcriptCatchUpDue(current, Date.now() - heardAt.current)) {
        return
      }
      heardAt.current = Date.now()
      void useChatStore.getState().catchUpTranscript(activeSessionId)
    }, 500)
    return () => window.clearInterval(timer)
  }, [activeSessionId, draftSelected, phase])
}
