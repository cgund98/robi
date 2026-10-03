import { useCallback, useEffect, useRef } from 'react'

import { getMessage } from '../api/messages'
import { ApiError } from '../api/sessions'
import { useReconnectingEventSource } from '../infra/useReconnectingEventSource'
import { useChatStore } from '../state/chatStore'
import { useIndexStore } from '../state/indexStore'
import { useWorkspaceStore } from '../state/workspaceStore'
import { postApprovalNotice, releaseApprovalPause } from './approvalNotice'
import { AGENT_EVENT_TYPES, buildAgentEventsStreamUrl, parseEventEnvelope } from './agentEvents'
import { fetchStillCurrent, startFetch } from './latestFetch'
import { transcriptCatchUpDue } from './transcriptCatchUp'

type DeltaData = {
  session_id?: string
  message_id?: string
  tool_call_id?: string
  delta?: { kind?: string }
  outcome?: { kind?: string; message?: string }
}

function indexStatus(data: unknown): {
  state: 'downloading' | 'indexing' | 'ready' | 'paused' | 'failed'
  files_done: number
  files_total: number
  error: string | null
} | null {
  if (!data || typeof data !== 'object') {
    return null
  }
  const record = data as Record<string, unknown>
  const state = record.state
  if (
    state !== 'downloading' &&
    state !== 'indexing' &&
    state !== 'ready' &&
    state !== 'paused' &&
    state !== 'failed'
  ) {
    return null
  }
  return {
    state,
    files_done: typeof record.files_done === 'number' ? record.files_done : 0,
    files_total: typeof record.files_total === 'number' ? record.files_total : 0,
    error: typeof record.error === 'string' ? record.error : null
  }
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
      const status = indexStatus(envelope.data)
      if (status) {
        useIndexStore.getState().apply(envelope.subject, status)
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
      case 'robi.agent.v1.session_updated':
        void useChatStore.getState().refreshSession(sessionId)
        return
      default:
        return
    }
  }, [])

  const onOpen = useCallback(() => {
    void useChatStore.getState().hydrateFromStream()
    const workspaceId = useWorkspaceStore.getState().activeWorkspaceId
    if (workspaceId) {
      void useIndexStore.getState().refresh(workspaceId)
    }
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
