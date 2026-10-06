import { createEffect, onCleanup } from 'solid-js'

import { getMessage } from '../../api/messages'
import { ApiError } from '../../api/sessions'
import { chat, patchChat } from '../../state/chatStore'
import { errors } from '../../state/errorLog'
import { index } from '../../state/indexStore'
import { mcp } from '../../state/mcpStore'
import { workspaces } from '../../state/workspaceStore'
import { postApprovalNotice, postTurnFailedNotice, releaseApprovalPause } from './approvalNotice'
import { AGENT_EVENT_TYPES, buildAgentEventsStreamUrl, parseEventEnvelope } from './agentEvents'
import { fetchStillCurrent, startFetch } from '../../app/latestFetch'
import { transcriptCatchUpDue } from './transcriptCatchUp'

type DeltaData = {
  session_id?: string
  message_id?: string
  tool_call_id?: string
  message?: string
  /** New session title on a `robi.session.v1.updated` frame. */
  title?: string
  /** Display summary on a `robi.session.v1.updated` frame. */
  turn_display?: string
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
      chat.upsertMessage(sessionId, message)
    })
    .catch((err: unknown) => {
      if (!fetchStillCurrent(key, generation)) {
        return
      }
      if (err instanceof ApiError && err.status === 404) {
        return
      }
      const message = err instanceof Error ? err.message : 'Failed to load message'
      errors.report(message, sessionId)
      patchChat({ error: message })
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
    chat.bumpReview(sessionId)
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
      chat.bumpReview(sessionId)
    }, 300)
  )
}

/**
 * One event stream for the open workspace. Every session's frames arrive.
 * A message or transcript fetch runs only for the session on screen.
 */
export function useAgentEvents(): void {
  let heardAt = 0

  const onEvent = (event: MessageEvent) => {
    if (typeof event.data !== 'string') {
      return
    }
    const envelope = parseEventEnvelope(event.data)
    if (!envelope) {
      return
    }
    if (envelope.type === 'robi.index.v1.progress') {
      if (envelope.subject) {
        // The frame carries the same status object as the GET, so apply it
        // directly. A GET would be served from a 10-second cache and could
        // drop the final `ready` transition.
        const applied = index.applyFrame(envelope.subject, envelope.data)
        if (!applied) {
          void index.refresh(envelope.subject)
        }
      }
      return
    }
    if (envelope.type === 'robi.mcp.v1.status') {
      if (envelope.subject) {
        void mcp.refresh(envelope.subject)
      }
      return
    }
    const data = eventData(envelope.data)
    const sessionId = envelope.subject || data?.session_id
    if (!sessionId) {
      return
    }

    const viewing = () => !chat.draftSelected && chat.activeSessionId === sessionId
    if (viewing()) {
      heardAt = Date.now()
    }

    switch (envelope.type) {
      case 'robi.agent.v1.turn_started':
        releaseApprovalPause(sessionId)
        if (viewing()) {
          chat.setPhase(sessionId, 'thinking')
        } else {
          chat.noteRunning(sessionId, true)
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
          chat.noteDelta(sessionId, kind)
        }
        return
      }
      case 'robi.agent.v1.turn_finished': {
        if (!viewing()) {
          chat.noteRunning(sessionId, false)
          const outcome = data?.outcome
          if (outcome?.kind === 'failed') {
            const message = outcome.message ?? 'The turn failed'
            errors.report(message, sessionId)
            void postTurnFailedNotice(sessionId, message)
          }
          return
        }
        chat.bumpReview(sessionId)
        const outcome = data?.outcome
        const failed = outcome?.kind === 'failed' ? (outcome.message ?? 'The turn failed') : null
        if (failed) {
          void postTurnFailedNotice(sessionId, failed)
        }
        void chat.finishTurn(sessionId, failed)
        return
      }
      case 'robi.agent.v1.transcript_compacted': {
        if (!viewing()) {
          return
        }
        // Rows were deleted, so a single message refetch is not enough: reload
        // the whole transcript.
        void chat.catchUpTranscript(sessionId)
        return
      }
      case 'robi.session.v1.created':
      case 'robi.session.v1.updated': {
        const id = typeof data?.session_id === 'string' ? data.session_id : sessionId
        // An auto-generated or manual title arrives in the frame; apply it
        // without a refetch, which can race with an in-flight session read.
        const title = typeof data?.title === 'string' && data.title.length > 0 ? data.title : null
        const turnDisplay = typeof data?.turn_display === 'string' ? data.turn_display : null
        if (title) {
          chat.renameSessionLocal(id, title)
        }
        if (turnDisplay) {
          chat.noteTurnDisplay(id, turnDisplay)
        }
        if (turnDisplay || !title) {
          void chat.refreshSession(id)
        }
        return
      }
      case 'robi.session.v1.deleted': {
        const id = typeof data?.session_id === 'string' ? data.session_id : null
        if (id) {
          void chat.forgetSession(id)
        }
        return
      }
      case 'robi.app.v1.error': {
        const message = data?.message
        if (typeof message === 'string' && message.length > 0) {
          errors.report(message, null)
          patchChat({ error: message })
        }
        return
      }
      default:
        return
    }
  }

  createEffect(() => {
    const id = workspaces.activeWorkspaceId
    const url = id ? buildAgentEventsStreamUrl(null, undefined, id) : null
    if (!url) {
      return
    }
    let source: EventSource | null = null
    let closed = false
    let opened = false
    let attempt = 0
    let timer: ReturnType<typeof setTimeout> | undefined
    const connect = () => {
      if (closed) {
        return
      }
      const next = new EventSource(url)
      source = next
      next.onopen = () => {
        if (closed) {
          return
        }
        opened = true
        attempt = 0
        if (opened) {
          void chat.hydrateFromStream()
        }
      }
      next.onerror = () => {
        next.close()
        if (closed) {
          return
        }
        const delay = Math.min(500 * 2 ** attempt, 15_000)
        attempt += 1
        timer = setTimeout(connect, delay)
      }
      for (const eventType of AGENT_EVENT_TYPES) {
        next.addEventListener(eventType, (event) => {
          if (!closed) {
            onEvent(event as MessageEvent)
          }
        })
      }
    }
    connect()
    onCleanup(() => {
      closed = true
      clearTimeout(timer)
      source?.close()
    })
  })

  createEffect(() => {
    const sessionId = chat.activeSessionId
    const draft = chat.draftSelected
    const phase = sessionId ? chat.phaseBySession[sessionId] : undefined
    heardAt = Date.now()
    if (!sessionId || draft) {
      return
    }
    if (phase !== 'thinking' && phase !== 'responding') {
      return
    }
    const timer = window.setInterval(() => {
      const current = chat.phaseBySession[sessionId]
      if (!transcriptCatchUpDue(current, Date.now() - heardAt)) {
        return
      }
      heardAt = Date.now()
      void chat.catchUpTranscript(sessionId)
    }, 500)
    onCleanup(() => window.clearInterval(timer))
  })
}
