import { create } from 'zustand'

import {
  decideToolCall,
  listMessages,
  stopSession,
  submitInstruction,
  type ChatMessage
} from '../api/messages'
import {
  ApiError,
  createSession,
  deleteSession,
  getSession,
  listSessions,
  patchSession,
  updateSession,
  type AgentMode,
  type ChatSession,
  type ModelConfigBody
} from '../api/sessions'
import { fetchStillCurrent, startFetch } from '../app/latestFetch'
import { useWorkspaceStore } from './workspaceStore'

export type AgentPhase = 'idle' | 'thinking' | 'responding'

type PendingEcho = {
  sessionId: string
  text: string
}

type ChatState = {
  sessions: ChatSession[]
  activeSessionId: string | null
  draftSelected: boolean
  /** Mode for a chat that has no row yet. */
  draftMode: AgentMode
  /** Model override for a chat that has no row yet. Null inherits the setting. */
  draftModel: string | null
  /** Effort override for a chat that has no row yet. Null inherits the setting. */
  draftEffort: string | null
  messagesBySession: Record<string, ChatMessage[]>
  phaseBySession: Record<string, AgentPhase>
  pendingEcho: PendingEcho | null
  error: string | null
  loading: boolean
  busy: boolean
  /** Session whose stop request is in flight. Input stays locked until it returns. */
  stoppingSessionId: string | null
  loadSessions: (options?: { draft?: boolean }) => Promise<void>
  selectSession: (id: string) => Promise<void>
  selectDraft: () => void
  setModeChoice: (mode: AgentMode) => Promise<void>
  setModelChoice: (model: string | null) => Promise<void>
  setEffortChoice: (effort: string | null) => Promise<void>
  sendInstruction: (instruction: string) => Promise<boolean>
  stopAgent: () => Promise<void>
  decideCall: (sessionId: string, callId: string, decision: 'approve' | 'reject') => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  removeSession: (id: string) => Promise<void>
  upsertMessage: (sessionId: string, message: ChatMessage) => void
  setPhase: (sessionId: string, phase: AgentPhase) => void
  noteDelta: (sessionId: string, kind: string) => void
  finishTurn: (sessionId: string, failedMessage: string | null) => Promise<void>
  /** Reload the transcript while a turn looks busy, so a missed frame cannot hide it. */
  catchUpTranscript: (sessionId: string) => Promise<void>
  refreshSession: (sessionId: string) => Promise<void>
  hydrateFromStream: () => Promise<void>
  /** Bumped when a tool call or turn finishes, so the review strip refetches. */
  reviewTickBySession: Record<string, number>
  bumpReview: (sessionId: string) => void
}

let hydrateEpoch = 0
let stopTick = 0
const listTokenBySession = new Map<string, number>()

function bumpHydrate(): number {
  hydrateEpoch += 1
  return hydrateEpoch
}

function beginList(sessionId: string): number {
  const next = (listTokenBySession.get(sessionId) ?? 0) + 1
  listTokenBySession.set(sessionId, next)
  return next
}

function listIsCurrent(sessionId: string, token: number): boolean {
  return listTokenBySession.get(sessionId) === token
}

function errorText(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback
}

function omitRecordKey<T>(record: Record<string, T>, key: string): Record<string, T> {
  const next = { ...record }
  delete next[key]
  return next
}

function replaceSession(sessions: ChatSession[], session: ChatSession): ChatSession[] {
  const index = sessions.findIndex((item) => item.id === session.id)
  if (index < 0) {
    return [session, ...sessions]
  }
  const current = sessions[index]
  if (current.updated_at > session.updated_at) {
    return sessions
  }
  return sessions.map((item) => (item.id === session.id ? session : item))
}

function activeMode(state: ChatState): AgentMode {
  if (state.draftSelected || state.activeSessionId === null) {
    return state.draftMode
  }
  const session = state.sessions.find((item) => item.id === state.activeSessionId)
  return sessionMode(session)
}

export function sessionMode(session: Pick<ChatSession, 'mode'> | null | undefined): AgentMode {
  if (session?.mode === 'ask' || session?.mode === 'plan' || session?.mode === 'agent') {
    return session.mode
  }
  return 'agent'
}

function draftConfig(state: ChatState): { mode: AgentMode; modelConfig?: ModelConfigBody } {
  const mode = state.draftMode
  const override: { model?: string; reasoning_effort?: string } = {}
  if (state.draftModel) {
    override.model = state.draftModel
  }
  if (state.draftEffort) {
    override.reasoning_effort = state.draftEffort
  }
  if (override.model == null && override.reasoning_effort == null) {
    return { mode }
  }
  return { mode, modelConfig: { [mode]: override } }
}

async function setChoice(
  get: () => ChatState,
  set: (partial: Partial<ChatState> | ((state: ChatState) => Partial<ChatState>)) => void,
  key: 'model' | 'reasoning_effort',
  value: string | null
): Promise<void> {
  const state = get()
  const { draftSelected, activeSessionId } = state
  if (draftSelected || activeSessionId === null) {
    set(key === 'model' ? { draftModel: value } : { draftEffort: value })
    return
  }
  const mode = activeMode(state)
  try {
    const updated = await patchSession(activeSessionId, {
      model_config: { [mode]: { [key]: value } }
    })
    set((state) => ({
      sessions: replaceSession(state.sessions, updated),
      error: null
    }))
  } catch (err) {
    set({ error: errorText(err, 'Failed to update the model') })
  }
}

function withoutEcho(
  echo: PendingEcho | null,
  sessionId: string,
  previous: ChatMessage[],
  next: ChatMessage[]
): PendingEcho | null {
  if (!echo || echo.sessionId !== sessionId) {
    return echo
  }
  const copies = (list: ChatMessage[]) =>
    list.filter((message) => message.role === 'user' && message.content === echo.text).length
  return copies(next) > copies(previous) ? null : echo
}

function nextPhase(current: AgentPhase | undefined, hasPending: boolean): AgentPhase {
  if (!hasPending) {
    return 'idle'
  }
  if (current === 'thinking' || current === 'responding') {
    return current
  }
  return 'thinking'
}

function sessionKey(sessionId: string): string {
  return `session:${sessionId}`
}

function sessionsListKey(workspaceId: string): string {
  return `sessions:${workspaceId}`
}

async function readTranscript(
  sessionId: string,
  epoch: number
): Promise<{ messages: ChatMessage[] | null; session: ChatSession | null } | null> {
  const token = beginList(sessionId)
  const sessionGeneration = startFetch(sessionKey(sessionId))
  const [messagesResult, sessionResult] = await Promise.allSettled([
    listMessages(sessionId),
    getSession(sessionId)
  ])
  if (epoch !== hydrateEpoch) {
    return null
  }
  const listCurrent = listIsCurrent(sessionId, token)
  const sessionCurrent = fetchStillCurrent(sessionKey(sessionId), sessionGeneration)
  const failure = [messagesResult, sessionResult].find(
    (result, index) => result.status === 'rejected' && (index === 0 ? listCurrent : sessionCurrent)
  )
  if (failure?.status === 'rejected') {
    throw failure.reason
  }
  const currentMessages =
    listCurrent && messagesResult.status === 'fulfilled' ? messagesResult.value : null
  const currentSession =
    sessionCurrent && sessionResult.status === 'fulfilled' ? sessionResult.value : null
  if (!currentMessages && !currentSession) {
    return null
  }
  return { messages: currentMessages, session: currentSession }
}

function applyTranscript(
  sessionId: string,
  messages: ChatMessage[] | null,
  session: ChatSession | null
): void {
  useChatStore.setState((state) => {
    const previous = state.messagesBySession[sessionId] ?? []
    return {
      sessions: session ? replaceSession(state.sessions, session) : state.sessions,
      messagesBySession: messages
        ? { ...state.messagesBySession, [sessionId]: messages }
        : state.messagesBySession,
      pendingEcho: messages
        ? withoutEcho(state.pendingEcho, sessionId, previous, messages)
        : state.pendingEcho,
      phaseBySession: session
        ? {
            ...state.phaseBySession,
            [sessionId]: nextPhase(state.phaseBySession[sessionId], session.has_pending_agent)
          }
        : state.phaseBySession
    }
  })
}

export const useChatStore = create<ChatState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  draftSelected: false,
  draftMode: 'agent',
  draftModel: null,
  draftEffort: null,
  messagesBySession: {},
  phaseBySession: {},
  pendingEcho: null,
  error: null,
  loading: true,
  busy: false,
  stoppingSessionId: null,
  reviewTickBySession: {},

  loadSessions: async (options) => {
    const epoch = bumpHydrate()
    const workspaceId = useWorkspaceStore.getState().activeWorkspaceId
    const firstPaint = get().sessions.length === 0
    set({ loading: firstPaint, busy: !firstPaint, error: null })
    if (!workspaceId) {
      if (epoch !== hydrateEpoch) {
        return
      }
      set({
        sessions: [],
        loading: false,
        busy: false,
        draftSelected: true,
        activeSessionId: null
      })
      return
    }
    const listGeneration = startFetch(sessionsListKey(workspaceId))
    try {
      const next = await listSessions(workspaceId)
      if (epoch !== hydrateEpoch) {
        return
      }
      if (!fetchStillCurrent(sessionsListKey(workspaceId), listGeneration)) {
        set({ loading: false, busy: false })
        return
      }
      if (options?.draft) {
        set({
          sessions: next,
          loading: false,
          busy: false,
          draftSelected: true,
          activeSessionId: null
        })
        return
      }
      const currentId = get().activeSessionId
      const keep = currentId !== null && next.some((session) => session.id === currentId)
      set({ sessions: next, loading: false, busy: false })
      if (keep && currentId) {
        const transcript = await readTranscript(currentId, epoch)
        if (transcript) {
          applyTranscript(currentId, transcript.messages, transcript.session)
        }
        return
      }
      const fallback = next[0]
      if (!fallback) {
        set({ draftSelected: true, activeSessionId: null })
        return
      }
      set({ activeSessionId: fallback.id, draftSelected: false })
      const transcript = await readTranscript(fallback.id, epoch)
      if (transcript) {
        applyTranscript(fallback.id, transcript.messages, transcript.session)
      }
    } catch (err) {
      if (epoch !== hydrateEpoch) {
        return
      }
      if (!fetchStillCurrent(sessionsListKey(workspaceId), listGeneration)) {
        set({ loading: false, busy: false })
        return
      }
      set({
        loading: false,
        busy: false,
        error: errorText(err, 'Failed to load chat sessions'),
        draftSelected: get().activeSessionId ? get().draftSelected : true
      })
    }
  },

  selectSession: async (id) => {
    if (get().activeSessionId === id && !get().draftSelected) {
      return
    }
    const epoch = bumpHydrate()
    set({ activeSessionId: id, draftSelected: false, error: null })
    try {
      const transcript = await readTranscript(id, epoch)
      if (!transcript) {
        return
      }
      applyTranscript(id, transcript.messages, transcript.session)
    } catch (err) {
      if (epoch !== hydrateEpoch) {
        return
      }
      set({ error: errorText(err, 'Failed to load chat session') })
    }
  },

  selectDraft: () => {
    if (get().draftSelected) {
      return
    }
    bumpHydrate()
    set({
      draftSelected: true,
      activeSessionId: null,
      error: null,
      draftMode: 'agent',
      draftModel: null,
      draftEffort: null
    })
  },

  setModeChoice: async (mode) => {
    const { draftSelected, activeSessionId } = get()
    if (draftSelected || activeSessionId === null) {
      set({ draftMode: mode })
      return
    }
    try {
      const updated = await patchSession(activeSessionId, { mode })
      set((state) => ({
        sessions: replaceSession(state.sessions, updated),
        error: null
      }))
    } catch (err) {
      set({ error: errorText(err, 'Failed to update the mode') })
    }
  },

  setModelChoice: async (model) => {
    await setChoice(get, set, 'model', model)
  },

  setEffortChoice: async (effort) => {
    await setChoice(get, set, 'reasoning_effort', effort)
  },

  sendInstruction: async (instruction) => {
    const text = instruction.trim()
    if (!text || get().busy) {
      return false
    }
    const { draftSelected, activeSessionId } = get()
    const creating = draftSelected || activeSessionId === null
    if (!creating) {
      const phase = get().phaseBySession[activeSessionId] ?? 'idle'
      if (phase !== 'idle') {
        return false
      }
    }

    stopTick += 1
    set({ busy: true, error: null })
    try {
      if (creating) {
        const workspaceId = useWorkspaceStore.getState().activeWorkspaceId
        if (!workspaceId) {
          set({ busy: false, error: 'Choose a workspace first' })
          return false
        }
        const created = await createSession(workspaceId, undefined, draftConfig(get()))
        const epoch = bumpHydrate()
        set((state) => ({
          sessions: [created, ...state.sessions.filter((session) => session.id !== created.id)],
          activeSessionId: created.id,
          draftSelected: false,
          draftMode: 'agent',
          draftModel: null,
          draftEffort: null
        }))
        try {
          await submitInstruction(created.id, text)
        } catch (err) {
          if (epoch !== hydrateEpoch) {
            set({ busy: false })
            return false
          }
          set({ busy: false, error: errorText(err, 'Failed to send message') })
          return false
        }
        if (epoch !== hydrateEpoch) {
          set({ busy: false })
          return false
        }
        set((state) => ({
          busy: false,
          pendingEcho: { sessionId: created.id, text },
          phaseBySession: { ...state.phaseBySession, [created.id]: 'thinking' }
        }))
        return true
      }

      await submitInstruction(activeSessionId, text)
      set((state) => ({
        busy: false,
        pendingEcho: { sessionId: activeSessionId, text },
        phaseBySession: { ...state.phaseBySession, [activeSessionId]: 'thinking' }
      }))
      return true
    } catch (err) {
      set({ busy: false, error: errorText(err, 'Failed to send message') })
      return false
    }
  },

  stopAgent: async () => {
    const { draftSelected, activeSessionId, stoppingSessionId } = get()
    if (draftSelected || activeSessionId === null || stoppingSessionId === activeSessionId) {
      return
    }
    const phase = get().phaseBySession[activeSessionId] ?? 'idle'
    if (phase === 'idle') {
      return
    }
    const sessionId = activeSessionId
    set({ stoppingSessionId: sessionId, error: null })
    try {
      await stopSession(sessionId)
      stopTick += 1
      const tick = stopTick
      const epoch = hydrateEpoch
      set((state) => ({
        stoppingSessionId: state.stoppingSessionId === sessionId ? null : state.stoppingSessionId,
        phaseBySession: { ...state.phaseBySession, [sessionId]: 'idle' }
      }))
      const transcript = await readTranscript(sessionId, epoch)
      if (tick !== stopTick || epoch !== hydrateEpoch) {
        return
      }
      if (transcript) {
        applyTranscript(sessionId, transcript.messages, transcript.session)
        get().bumpReview(sessionId)
      }
      set((state) => ({
        pendingEcho: state.pendingEcho?.sessionId === sessionId ? null : state.pendingEcho
      }))
    } catch (err) {
      set((state) => ({
        stoppingSessionId: state.stoppingSessionId === sessionId ? null : state.stoppingSessionId,
        error: errorText(err, 'Failed to stop')
      }))
    }
  },

  decideCall: async (sessionId, callId, decision) => {
    set((state) => ({
      busy: true,
      error: null,
      phaseBySession: { ...state.phaseBySession, [sessionId]: 'thinking' }
    }))
    try {
      await decideToolCall(sessionId, callId, decision)
      set({ busy: false })
    } catch (err) {
      set((state) => ({
        busy: false,
        error: errorText(err, 'Failed to settle the tool call'),
        phaseBySession: { ...state.phaseBySession, [sessionId]: 'idle' }
      }))
    }
  },

  renameSession: async (id, title) => {
    set({ busy: true, error: null })
    try {
      const updated = await updateSession(id, title)
      set((state) => ({
        busy: false,
        sessions: state.sessions.map((session) => (session.id === id ? updated : session))
      }))
    } catch (err) {
      set({ busy: false, error: errorText(err, 'Failed to rename chat session') })
    }
  },

  removeSession: async (id) => {
    set({ busy: true, error: null })
    try {
      await deleteSession(id)
    } catch (err) {
      set({ busy: false, error: errorText(err, 'Failed to delete chat session') })
      return
    }

    const epoch = bumpHydrate()
    const next = get().sessions.filter((session) => session.id !== id)
    const wasActive = get().activeSessionId === id && !get().draftSelected
    set((state) => ({
      busy: false,
      sessions: next,
      messagesBySession: omitRecordKey(state.messagesBySession, id),
      phaseBySession: omitRecordKey(state.phaseBySession, id),
      pendingEcho: state.pendingEcho?.sessionId === id ? null : state.pendingEcho,
      activeSessionId: wasActive ? null : state.activeSessionId,
      draftSelected: wasActive ? next.length === 0 : state.draftSelected
    }))
    if (!wasActive) {
      return
    }
    const fallback = next[0]
    if (!fallback) {
      set({ draftSelected: true, activeSessionId: null })
      return
    }
    set({ activeSessionId: fallback.id, draftSelected: false })
    try {
      const transcript = await readTranscript(fallback.id, epoch)
      if (transcript) {
        applyTranscript(fallback.id, transcript.messages, transcript.session)
      }
    } catch (err) {
      if (epoch !== hydrateEpoch) {
        return
      }
      set({ error: errorText(err, 'Failed to load chat session') })
    }
  },

  upsertMessage: (sessionId, message) => {
    beginList(sessionId)
    set((state) => {
      const current = state.messagesBySession[sessionId] ?? []
      const index = current.findIndex((item) => item.id === message.id)
      const messages =
        index >= 0
          ? current.map((item) => (item.id === message.id ? message : item))
          : [...current, message]
      return {
        messagesBySession: { ...state.messagesBySession, [sessionId]: messages },
        pendingEcho: withoutEcho(state.pendingEcho, sessionId, current, messages)
      }
    })
  },

  setPhase: (sessionId, phase) => {
    set((state) => {
      if (state.phaseBySession[sessionId] === phase) {
        return state
      }
      return {
        phaseBySession: { ...state.phaseBySession, [sessionId]: phase }
      }
    })
  },

  noteDelta: (sessionId, kind) => {
    if (kind !== 'text' && kind !== 'reasoning') {
      return
    }
    const phase: AgentPhase = kind === 'text' ? 'responding' : 'thinking'
    set((state) => {
      if (state.phaseBySession[sessionId] === phase) {
        return state
      }
      return {
        phaseBySession: { ...state.phaseBySession, [sessionId]: phase }
      }
    })
  },

  finishTurn: async (sessionId, failedMessage) => {
    const tick = stopTick
    const epoch = hydrateEpoch
    const stopping = get().stoppingSessionId === sessionId
    if (!stopping) {
      set((state) => ({
        phaseBySession: { ...state.phaseBySession, [sessionId]: 'idle' },
        error: failedMessage ?? state.error
      }))
    } else if (failedMessage) {
      set({ error: failedMessage })
    }
    try {
      const transcript = await readTranscript(sessionId, epoch)
      if (tick !== stopTick || epoch !== hydrateEpoch || !transcript) {
        return
      }
      applyTranscript(sessionId, transcript.messages, transcript.session)
      // The review fetch on the event can lose to a request that started
      // before the writes landed. Check again once this turn is stored.
      get().bumpReview(sessionId)
      if (failedMessage) {
        set({ error: failedMessage })
      }
      // turn_finished is emitted before the actor clears `running`, so this
      // refetch can still see has_pending_agent and put the phase back on
      // Thinking after the turn is stored. Read the session once more.
      if (
        transcript.session?.has_pending_agent &&
        get().stoppingSessionId !== sessionId &&
        get().phaseBySession[sessionId] !== 'idle'
      ) {
        const againGeneration = startFetch(sessionKey(sessionId))
        const again = await getSession(sessionId)
        if (
          tick !== stopTick ||
          epoch !== hydrateEpoch ||
          get().stoppingSessionId === sessionId ||
          !fetchStillCurrent(sessionKey(sessionId), againGeneration)
        ) {
          return
        }
        if (!again.has_pending_agent) {
          set((state) => ({
            sessions: replaceSession(state.sessions, again),
            phaseBySession: { ...state.phaseBySession, [sessionId]: 'idle' }
          }))
        }
      }
    } catch (err) {
      if (epoch !== hydrateEpoch) {
        return
      }
      set({ error: errorText(err, 'Failed to load chat session') })
    }
  },

  catchUpTranscript: async (sessionId) => {
    const epoch = hydrateEpoch
    const phase = get().phaseBySession[sessionId]
    if (phase !== 'thinking' && phase !== 'responding') {
      return
    }
    if (get().stoppingSessionId === sessionId) {
      return
    }
    try {
      const transcript = await readTranscript(sessionId, epoch)
      if (!transcript || epoch !== hydrateEpoch || get().stoppingSessionId === sessionId) {
        return
      }
      const current = get().phaseBySession[sessionId]
      if (current !== 'thinking' && current !== 'responding') {
        return
      }
      applyTranscript(sessionId, transcript.messages, transcript.session)
      get().bumpReview(sessionId)
    } catch (err) {
      if (epoch !== hydrateEpoch) {
        return
      }
      if (err instanceof ApiError && err.status === 404) {
        return
      }
    }
  },

  refreshSession: async (sessionId) => {
    const epoch = hydrateEpoch
    const generation = startFetch(sessionKey(sessionId))
    try {
      const session = await getSession(sessionId)
      if (epoch !== hydrateEpoch || !fetchStillCurrent(sessionKey(sessionId), generation)) {
        return
      }
      set((state) => ({
        sessions: replaceSession(state.sessions, session)
      }))
    } catch (err) {
      if (epoch !== hydrateEpoch || !fetchStillCurrent(sessionKey(sessionId), generation)) {
        return
      }
      if (err instanceof ApiError && err.status === 404) {
        return
      }
      set({ error: errorText(err, 'Failed to load chat session') })
    }
  },

  hydrateFromStream: async () => {
    const epoch = hydrateEpoch
    const { draftSelected, activeSessionId } = get()
    const workspaceId = useWorkspaceStore.getState().activeWorkspaceId
    if (!workspaceId) {
      return
    }
    const listGeneration = startFetch(sessionsListKey(workspaceId))
    try {
      const next = await listSessions(workspaceId)
      if (
        epoch !== hydrateEpoch ||
        !fetchStillCurrent(sessionsListKey(workspaceId), listGeneration)
      ) {
        return
      }
      set({ sessions: next })
      if (draftSelected || !activeSessionId) {
        return
      }
      if (!next.some((session) => session.id === activeSessionId)) {
        const fallback = next[0]
        if (!fallback) {
          set({ draftSelected: true, activeSessionId: null })
          return
        }
        set({ activeSessionId: fallback.id, draftSelected: false })
        const transcript = await readTranscript(fallback.id, epoch)
        if (transcript) {
          applyTranscript(fallback.id, transcript.messages, transcript.session)
          get().bumpReview(fallback.id)
        }
        return
      }
      const transcript = await readTranscript(activeSessionId, epoch)
      if (transcript) {
        applyTranscript(activeSessionId, transcript.messages, transcript.session)
        get().bumpReview(activeSessionId)
      }
    } catch (err) {
      if (
        epoch !== hydrateEpoch ||
        !fetchStillCurrent(sessionsListKey(workspaceId), listGeneration)
      ) {
        return
      }
      set({ error: errorText(err, 'Failed to load chat sessions') })
    }
  },

  bumpReview: (sessionId) => {
    set((state) => ({
      reviewTickBySession: {
        ...state.reviewTickBySession,
        [sessionId]: (state.reviewTickBySession[sessionId] ?? 0) + 1
      }
    }))
  }
}))
