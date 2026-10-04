import { useCallback, useEffect, useRef } from 'react'

import { getMessage } from '../api/messages'
import { ApiError } from '../api/sessions'
import { useReconnectingEventSource } from '../infra/useReconnectingEventSource'
import { useChatStore } from '../state/chatStore'
import { useErrorLog } from '../state/errorLog'
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
  /** New session title on a `robi.session.v1.updated` frame. */
  title?: string
  delta?: { kind?: string }
  outcome?: { kind?: string; message?: string }
}

function eventData(data: unknown): DeltaData | null {
  if (!data || typeof data !== 'object') {
    return null
  }
  return data as DeltaData
}

const messageFetchInflight = new Set<string>()
const messageFetchDirty = new Set<string>()

/** One message GET in flight, plus one trailing GET if a newer frame arrived. */
function requestMessage(sessionId: string, messageId: string): void {
  const key = `message:${sessionId}:${messageId}`
  if (messageFetchInflight.has(key)) {
    messageFetchDirty.add(key)
    return
  }
  messageFetchInflight.add(key)
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
      useErrorLog.getState().report(message, sessionId)
      useChatStore.setState({ error: message })
    })
    .finally(() => {
      messageFetchInflight.delete(key)
      if (messageFetchDirty.delete(key)) {
        requestMessage(sessionId, messageId)
      }
    })
}

const reviewArm = new Map<string, number>()

/** Leading review refresh, then one more once a burst of tool frames goes quiet. */
function bumpReviewSoon(sessionId: string): void {
  const armed = reviewArm.get(sessionId)
  if (armed === undefined) {
    useChatStore.getState().bumpReview(sessionId)
    reviewArm.set(
      sessionId,
      window.setTimeout(() => {
        reviewArm.delete(sessionId)
      }, 300)
    )
    return
  }
  window.clearTimeout(armed)
  reviewArm.set(
    sessionId,
    window.setTimeout(() => {
      reviewArm.delete(sessionId)
      useChatStore.getState().bumpReview(sessionId)
    }, 300)
  )
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
    const data = eventData(envelope.data)
    const sessionId = envelope.subject || data?.session_id
    if (!sessionId) {
      return
    }

    const viewing = () => {
      const state = useChatStore.getState()
      return !state.draftSelected && state.activeSessionId === sessionId
    }
    if (viewing()) {
      heardAt.current = Date.now()
    }

    switch (envelope.type) {
      case 'robi.agent.v1.turn_started':
        releaseApprovalPause(sessionId)
        if (viewing()) {
          useChatStore.getState().setPhase(sessionId, 'thinking')
        } else {
          useChatStore.getState().noteRunning(sessionId, true)
        }
        return
      case 'robi.agent.v1.awaiting_approval':
        void postApprovalNotice(sessionId, data?.tool_call_id)
        return
      case 'robi.agent.v1.message_added':
      case 'robi.agent.v1.message_updated':
      case 'robi.agent.v1.tool_call_updated': {
        if (!viewing()) {
          return
        }
        if (envelope.type === 'robi.agent.v1.tool_call_updated') {
          bumpReviewSoon(sessionId)
        }
        const messageId = data?.message_id
        if (!messageId) {
          return
        }
        requestMessage(sessionId, messageId)
        return
      }
      case 'robi.agent.v1.message_delta': {
        if (!viewing()) {
          return
        }
        const kind = data?.delta?.kind
        if (kind) {
          useChatStore.getState().noteDelta(sessionId, kind)
        }
        return
      }
      case 'robi.agent.v1.turn_finished': {
        if (!viewing()) {
          useChatStore.getState().noteRunning(sessionId, false)
          const outcome = data?.outcome
          if (outcome?.kind === 'failed') {
            useErrorLog.getState().report(outcome.message ?? 'The turn failed', sessionId)
          }
          return
        }
        useChatStore.getState().bumpReview(sessionId)
        const outcome = data?.outcome
        const failed = outcome?.kind === 'failed' ? (outcome.message ?? 'The turn failed') : null
        void useChatStore.getState().finishTurn(sessionId, failed)
        return
      }
      case 'robi.session.v1.created':
      case 'robi.session.v1.updated': {
        const id = typeof data?.session_id === 'string' ? data.session_id : sessionId
        // An auto-generated or manual title arrives in the frame; apply it
        // without a refetch, which can race with an in-flight session read.
        const title = typeof data?.title === 'string' && data.title.length > 0 ? data.title : null
        if (title) {
          useChatStore.getState().renameSessionLocal(id, title)
        } else {
          void useChatStore.getState().refreshSession(id)
        }
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
          useErrorLog.getState().report(message, null)
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
