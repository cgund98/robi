import {
  compactSession,
  decideToolCall,
  getMessage,
  listMessages,
  stopSession,
  submitInstruction,
  type ChatMessage
} from '../api/messages'
import {
  toFileInputs,
  type AttachmentMeta,
  type FileAttachment
} from '../features/chat/textAttachments'
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
import { dropSentComposerDraft } from './composerDrafts'
import { errors } from './errorLog'
import {
  mountStore,
  type ActiveWorkspace,
  type ErrorReporter,
  type StoreGet,
  type StoreSet
} from './storeDeps'
import { workspaces } from './workspaceStore'

export type ChatDeps = ErrorReporter & ActiveWorkspace

function noteError(deps: ChatDeps, message: string, sessionId?: string | null): string {
  if (!isAbortMessage(message)) {
    deps.reportError(message, sessionId ?? null)
  }
  return message
}

function isAbortMessage(message: string): boolean {
  return message === 'Fetch is aborted' || message === 'The operation was aborted.'
}

export type AgentPhase = 'idle' | 'thinking' | 'responding'

type PendingEcho = {
  sessionId: string
  text: string
  files: AttachmentMeta[]
}

export type ChatState = {
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
  /** True while the first fetch of the selected session's transcript is in flight. */
  transcriptLoading: boolean
  busy: boolean
  /** Session whose stop request is in flight. Input stays locked until it returns. */
  stoppingSessionId: string | null
  /** Session whose compact request is in flight. The Compact button is disabled until it returns. */
  compactingSessionId: string | null
  loadSessions: (options?: { draft?: boolean }) => Promise<void>
  selectSession: (id: string) => Promise<void>
  selectDraft: () => void
  setModeChoice: (mode: AgentMode) => Promise<void>
  setModelChoice: (model: string | null) => Promise<void>
  setEffortChoice: (effort: string | null) => Promise<void>
  sendInstruction: (
    instruction: string,
    images?: File[],
    files?: FileAttachment[]
  ) => Promise<boolean>
  stopAgent: () => Promise<void>
  /** Ask the active session to summarize its older prefix. */
  compactAgent: () => Promise<void>
  decideCall: (sessionId: string, callId: string, decision: 'approve' | 'reject') => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  removeSession: (id: string) => Promise<void>
  /** Drop a session another window deleted. Does not call the API. */
  forgetSession: (id: string) => Promise<void>
  upsertMessage: (sessionId: string, message: ChatMessage) => void
  /** Sidebar running mark for a session that is not on screen. No transcript fetch. */
  noteRunning: (sessionId: string, running: boolean) => void
  setPhase: (sessionId: string, phase: AgentPhase) => void
  noteDelta: (sessionId: string, kind: string) => void
  finishTurn: (sessionId: string, failedMessage: string | null) => Promise<void>
  /** Reload the transcript while a turn looks busy, so a missed frame cannot hide it. */
  catchUpTranscript: (sessionId: string) => Promise<void>
  refreshSession: (sessionId: string) => Promise<void>
  /** Apply a title delivered on an SSE frame without a refetch. */
  renameSessionLocal: (sessionId: string, title: string) => void
  /** Apply `turn_display` from an SSE frame before the session refetch returns. */
  noteTurnDisplay: (sessionId: string, turnDisplay: string) => void
  hydrateFromStream: () => Promise<void>
  /** Bumped when a tool call or turn finishes, so the review strip refetches. */
  reviewTickBySession: Record<string, number>
  bumpReview: (sessionId: string) => void
}

let hydrateEpoch = 0
let stopTick = 0
let localRevision = 0

/** Per-message stamp. A list fetch keeps a row stamped after the fetch started. */
const messageRevision = new Map<string, number>()
/** Session rows inserted locally after a list fetch started stay in the list. */
const sessionInsertedAt = new Map<string, number>()
/** Title applied locally. A fetch that started earlier keeps this title. */
const titleRevision = new Map<string, number>()
/** `has_pending_agent` written by a turn frame. A fetch that started earlier keeps it. */
const pendingRevision = new Map<string, number>()
/** `turn_display` written from an event. A fetch that started earlier keeps it. */
const turnDisplayRevision = new Map<string, number>()

const transcriptFlight = new Map<string, Promise<TranscriptRead | null>>()

function bumpHydrate(): number {
  hydrateEpoch += 1
  return hydrateEpoch
}

function bumpRevision(): number {
  localRevision += 1
  return localRevision
}

function revisionNow(): number {
  return localRevision
}

function messageStampKey(sessionId: string, messageId: string): string {
  return `${sessionId}:${messageId}`
}

function stampMessage(sessionId: string, messageId: string): void {
  messageRevision.set(messageStampKey(sessionId, messageId), bumpRevision())
}

function stampSessionInserted(sessionId: string): void {
  sessionInsertedAt.set(sessionId, bumpRevision())
}

function stampTitle(sessionId: string): void {
  titleRevision.set(sessionId, bumpRevision())
}

function stampPending(sessionId: string): void {
  pendingRevision.set(sessionId, bumpRevision())
}

/** Reload the assistant row that owns this call. A failed load keeps the row we have. */
async function refreshCallMessage(
  get: () => ChatState,
  sessionId: string,
  callId: string
): Promise<ChatMessage | null> {
  const current = get().messagesBySession[sessionId]?.find((item) =>
    item.tool_calls.some((call) => call.id === callId)
  )
  if (!current) {
    return null
  }
  try {
    const fresh = await getMessage(sessionId, current.id)
    get().upsertMessage(sessionId, fresh)
    return fresh
  } catch {
    return current
  }
}

function callStillOpen(message: ChatMessage | null, callId: string): boolean {
  if (!message) {
    return true
  }
  const call = message.tool_calls.find((item) => item.id === callId)
  if (!call) {
    return true
  }
  return call.approval_status === 'pending' && call.execution_status === 'not_started'
}

function errorText(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback
}

function omitRecordKey<T>(record: Record<string, T>, key: string): Record<string, T> {
  const next = { ...record }
  delete next[key]
  return next
}

function replaceSession(
  sessions: ChatSession[],
  session: ChatSession,
  seenAt: number
): ChatSession[] {
  const index = sessions.findIndex((item) => item.id === session.id)
  if (index < 0) {
    return [session, ...sessions]
  }
  const current = sessions[index]
  let next = session
  if ((titleRevision.get(session.id) ?? 0) > seenAt) {
    next = { ...next, title: current.title }
  }
  if ((pendingRevision.get(session.id) ?? 0) > seenAt) {
    next = { ...next, has_pending_agent: current.has_pending_agent }
  }
  if ((turnDisplayRevision.get(session.id) ?? 0) > seenAt) {
    next = {
      ...next,
      turn_display: current.turn_display,
      has_pending_agent:
        current.turn_display === 'awaiting_approval' ? false : next.has_pending_agent
    }
  }
  if (current.updated_at > next.updated_at) {
    return sessions
  }
  if (sameSessionRow(current, next)) {
    return sessions
  }
  return sessions.map((item) => (item.id === session.id ? next : item))
}

/** Server order, plus a local insert the response could not have contained. */
function mergeSessionList(
  local: ChatSession[],
  fetched: ChatSession[],
  seenAt: number
): ChatSession[] {
  let sessions = local
  for (const session of fetched) {
    sessions = replaceSession(sessions, session, seenAt)
  }
  const fetchedIds = new Set(fetched.map((session) => session.id))
  const extras = sessions.filter(
    (session) => !fetchedIds.has(session.id) && (sessionInsertedAt.get(session.id) ?? 0) > seenAt
  )
  const ordered = [...extras]
  for (const session of fetched) {
    const row = sessions.find((item) => item.id === session.id)
    if (row) {
      ordered.push(row)
    }
  }
  return ordered
}

/** Fields the sidebar and composer read. Timestamps alone do not refresh a row. */
function sameSessionRow(current: ChatSession, next: ChatSession): boolean {
  return (
    current.title === next.title &&
    current.mode === next.mode &&
    current.has_pending_agent === next.has_pending_agent &&
    current.turn_display === next.turn_display &&
    JSON.stringify(current.model_config) === JSON.stringify(next.model_config) &&
    JSON.stringify(current.allow_hosts) === JSON.stringify(next.allow_hosts) &&
    JSON.stringify(current.path_allow_read) === JSON.stringify(next.path_allow_read) &&
    JSON.stringify(current.path_allow_write) === JSON.stringify(next.path_allow_write) &&
    JSON.stringify(current.path_deny_read) === JSON.stringify(next.path_deny_read) &&
    JSON.stringify(current.path_deny_write) === JSON.stringify(next.path_deny_write)
  )
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
  deps: ChatDeps,
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
    const seenAt = revisionNow()
    const updated = await patchSession(activeSessionId, {
      model_config: { [mode]: { [key]: value } }
    })
    set((state) => ({
      sessions: replaceSession(state.sessions, updated, seenAt),
      error: null
    }))
  } catch (err) {
    set({ error: noteError(deps, errorText(err, 'Failed to update the model'), activeSessionId) })
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

function nextPhase(
  current: AgentPhase | undefined,
  hasPending: boolean,
  sessionId: string,
  seenAt: number
): AgentPhase {
  // A send marks the session running before the actor is visible to a read
  // that is already in flight. That read must not put the phase back to idle.
  if (
    (pendingRevision.get(sessionId) ?? 0) > seenAt &&
    (current === 'thinking' || current === 'responding')
  ) {
    return current
  }
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

type TranscriptRead = {
  messages: ChatMessage[] | null
  session: ChatSession | null
  seenAt: number
}

type ApplyPhase = 'session' | 'preserve'

async function fetchTranscript(sessionId: string): Promise<TranscriptRead | null> {
  const seenAt = revisionNow()
  const sessionGeneration = startFetch(sessionKey(sessionId))
  const [messagesResult, sessionResult] = await Promise.allSettled([
    listMessages(sessionId),
    getSession(sessionId)
  ])
  const sessionCurrent = fetchStillCurrent(sessionKey(sessionId), sessionGeneration)
  if (messagesResult.status === 'rejected') {
    throw messagesResult.reason
  }
  if (sessionResult.status === 'rejected' && sessionCurrent) {
    throw sessionResult.reason
  }
  const currentMessages = messagesResult.status === 'fulfilled' ? messagesResult.value : null
  const currentSession =
    sessionCurrent && sessionResult.status === 'fulfilled' ? sessionResult.value : null
  if (!currentMessages && !currentSession) {
    return null
  }
  return { messages: currentMessages, session: currentSession, seenAt }
}

/** One list-and-session read per session while a read is already in flight. */
function readTranscript(sessionId: string): Promise<TranscriptRead | null> {
  const existing = transcriptFlight.get(sessionId)
  if (existing) {
    return existing
  }
  const flight = fetchTranscript(sessionId).finally(() => {
    if (transcriptFlight.get(sessionId) === flight) {
      transcriptFlight.delete(sessionId)
    }
  })
  transcriptFlight.set(sessionId, flight)
  return flight
}

function mergeMessages(
  sessionId: string,
  fetched: ChatMessage[],
  current: ChatMessage[],
  seenAt: number
): ChatMessage[] {
  const fetchedIds = new Set(fetched.map((message) => message.id))
  const merged = fetched.map((message) => {
    const revision = messageRevision.get(messageStampKey(sessionId, message.id)) ?? 0
    if (revision > seenAt) {
      const local = current.find((item) => item.id === message.id)
      if (local) {
        return local
      }
    }
    return message
  })
  for (const message of current) {
    if (fetchedIds.has(message.id)) {
      continue
    }
    if ((messageRevision.get(messageStampKey(sessionId, message.id)) ?? 0) > seenAt) {
      merged.push(message)
    }
  }
  return merged
}

function applyTranscript(
  set: StoreSet<ChatState>,
  sessionId: string,
  messages: ChatMessage[] | null,
  session: ChatSession | null,
  seenAt: number,
  phaseMode: ApplyPhase = 'session'
): void {
  set((state) => {
    const previous = state.messagesBySession[sessionId] ?? []
    const nextMessages = messages ? mergeMessages(sessionId, messages, previous, seenAt) : null
    return {
      sessions: session ? replaceSession(state.sessions, session, seenAt) : state.sessions,
      messagesBySession: nextMessages
        ? { ...state.messagesBySession, [sessionId]: nextMessages }
        : state.messagesBySession,
      pendingEcho: nextMessages
        ? withoutEcho(state.pendingEcho, sessionId, previous, nextMessages)
        : state.pendingEcho,
      phaseBySession:
        phaseMode === 'session' && session
          ? {
              ...state.phaseBySession,
              [sessionId]: nextPhase(
                state.phaseBySession[sessionId],
                session.has_pending_agent,
                sessionId,
                seenAt
              )
            }
          : state.phaseBySession
    }
  })
}

export function createChatState(
  set: StoreSet<ChatState>,
  get: StoreGet<ChatState>,
  deps: ChatDeps
): ChatState {
  return {
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
    transcriptLoading: false,
    busy: false,
    stoppingSessionId: null,
    compactingSessionId: null,
    reviewTickBySession: {},

    loadSessions: async (options) => {
      const epoch = bumpHydrate()
      const workspaceId = deps.activeWorkspaceId()
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
          const transcript = await readTranscript(currentId)
          if (epoch !== hydrateEpoch || !transcript) {
            return
          }
          applyTranscript(
            set,
            currentId,
            transcript.messages,
            transcript.session,
            transcript.seenAt
          )
          get().bumpReview(currentId)
          return
        }
        const fallback = next[0]
        if (!fallback) {
          set({ draftSelected: true, activeSessionId: null })
          return
        }
        set({ activeSessionId: fallback.id, draftSelected: false })
        const transcript = await readTranscript(fallback.id)
        if (epoch !== hydrateEpoch || !transcript) {
          return
        }
        applyTranscript(
          set,
          fallback.id,
          transcript.messages,
          transcript.session,
          transcript.seenAt
        )
        get().bumpReview(fallback.id)
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
          error: noteError(deps, errorText(err, 'Failed to load chat sessions')),
          draftSelected: get().activeSessionId ? get().draftSelected : true
        })
      }
    },

    selectSession: async (id) => {
      if (get().activeSessionId === id && !get().draftSelected) {
        return
      }
      const epoch = bumpHydrate()
      const firstOpen = get().messagesBySession[id] === undefined
      set({
        activeSessionId: id,
        draftSelected: false,
        error: null,
        transcriptLoading: firstOpen
      })
      // Opening a session reloads its review, so the strip reflects this
      // session's files instead of whatever the previous one left behind.
      get().bumpReview(id)
      try {
        const transcript = await readTranscript(id)
        if (epoch !== hydrateEpoch) {
          return
        }
        if (transcript) {
          applyTranscript(set, id, transcript.messages, transcript.session, transcript.seenAt)
        }
        set({ transcriptLoading: false })
      } catch (err) {
        if (epoch !== hydrateEpoch) {
          return
        }
        set({
          transcriptLoading: false,
          error: noteError(deps, errorText(err, 'Failed to load chat session'), id)
        })
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
        transcriptLoading: false,
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
      const seenAt = revisionNow()
      try {
        const updated = await patchSession(activeSessionId, { mode })
        set((state) => ({
          sessions: replaceSession(state.sessions, updated, seenAt),
          error: null
        }))
      } catch (err) {
        set({
          error: noteError(deps, errorText(err, 'Failed to update the mode'), activeSessionId)
        })
      }
    },

    setModelChoice: async (model) => {
      await setChoice(deps, get, set, 'model', model)
    },

    setEffortChoice: async (effort) => {
      await setChoice(deps, get, set, 'reasoning_effort', effort)
    },

    sendInstruction: async (instruction, images, files) => {
      const text = instruction.trim()
      const hasImages = (images?.length ?? 0) > 0
      const hasFiles = (files?.length ?? 0) > 0
      if ((!text && !hasImages && !hasFiles) || get().busy) {
        return false
      }
      const echoFiles = (files ?? []).map((file) => ({
        name: file.name,
        path: file.path ?? file.absolutePath,
        startLine: file.startLine,
        endLine: file.endLine
      }))
      const fileInputs = hasFiles ? toFileInputs(files!) : undefined
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
          const workspaceId = deps.activeWorkspaceId()
          if (!workspaceId) {
            set({ busy: false, error: noteError(deps, 'Choose a workspace first') })
            return false
          }
          const created = await createSession(workspaceId, undefined, draftConfig(get()))
          stampSessionInserted(created.id)
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
            await submitInstruction(created.id, text, images, fileInputs)
          } catch (err) {
            if (epoch !== hydrateEpoch) {
              set({ busy: false })
              return false
            }
            set({
              busy: false,
              error: noteError(deps, errorText(err, 'Failed to send message'), activeSessionId)
            })
            return false
          }
          if (epoch !== hydrateEpoch) {
            set({ busy: false })
            return true
          }
          stampPending(created.id)
          dropSentComposerDraft(created.id, instruction)
          set((state) => ({
            busy: false,
            pendingEcho: { sessionId: created.id, text, files: echoFiles },
            phaseBySession: { ...state.phaseBySession, [created.id]: 'thinking' },
            sessions: state.sessions.map((item) =>
              item.id === created.id ? { ...item, has_pending_agent: true } : item
            )
          }))
          return true
        }

        await submitInstruction(activeSessionId, text, images, fileInputs)
        stampPending(activeSessionId)
        dropSentComposerDraft(activeSessionId, instruction)
        set((state) => ({
          busy: false,
          pendingEcho: { sessionId: activeSessionId, text, files: echoFiles },
          phaseBySession: { ...state.phaseBySession, [activeSessionId]: 'thinking' },
          sessions: state.sessions.map((item) =>
            item.id === activeSessionId ? { ...item, has_pending_agent: true } : item
          )
        }))
        return true
      } catch (err) {
        set({
          busy: false,
          error: noteError(deps, errorText(err, 'Failed to send message'), activeSessionId)
        })
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
        const transcript = await readTranscript(sessionId)
        if (tick !== stopTick || epoch !== hydrateEpoch) {
          return
        }
        if (transcript) {
          applyTranscript(
            set,
            sessionId,
            transcript.messages,
            transcript.session,
            transcript.seenAt,
            'preserve'
          )
          get().bumpReview(sessionId)
        }
        set((state) => ({
          pendingEcho: state.pendingEcho?.sessionId === sessionId ? null : state.pendingEcho
        }))
      } catch (err) {
        set((state) => ({
          stoppingSessionId: state.stoppingSessionId === sessionId ? null : state.stoppingSessionId,
          error: noteError(deps, errorText(err, 'Failed to stop'), sessionId)
        }))
      }
    },

    compactAgent: async () => {
      const { draftSelected, activeSessionId, compactingSessionId } = get()
      if (draftSelected || activeSessionId === null || compactingSessionId === activeSessionId) {
        return
      }
      const sessionId = activeSessionId
      set({ compactingSessionId: sessionId, error: null })
      try {
        await compactSession(sessionId)
        // The rewrite completes on the actor and arrives as `transcript_compacted`,
        // which refetches the transcript. Nothing else to do here.
        set((state) => ({
          compactingSessionId:
            state.compactingSessionId === sessionId ? null : state.compactingSessionId
        }))
      } catch (err) {
        set((state) => ({
          compactingSessionId:
            state.compactingSessionId === sessionId ? null : state.compactingSessionId,
          error: noteError(deps, errorText(err, 'Failed to compact the context'), sessionId)
        }))
      }
    },

    decideCall: async (sessionId, callId, decision) => {
      stampPending(sessionId)
      set((state) => ({
        busy: true,
        error: null,
        phaseBySession: { ...state.phaseBySession, [sessionId]: 'thinking' },
        sessions: state.sessions.map((item) =>
          item.id === sessionId ? { ...item, has_pending_agent: true } : item
        )
      }))
      try {
        await decideToolCall(sessionId, callId, decision)
        set({ busy: false })
      } catch (err) {
        const fresh = await refreshCallMessage(get, sessionId, callId)
        const stillOpen = callStillOpen(fresh, callId)
        if (stillOpen) {
          stampPending(sessionId)
        }
        set((state) => ({
          busy: false,
          error: stillOpen
            ? noteError(deps, errorText(err, 'Failed to settle the tool call'), sessionId)
            : state.error,
          phaseBySession: stillOpen
            ? { ...state.phaseBySession, [sessionId]: 'idle' }
            : state.phaseBySession,
          sessions: stillOpen
            ? state.sessions.map((item) =>
                item.id === sessionId ? { ...item, has_pending_agent: false } : item
              )
            : state.sessions
        }))
      }
    },

    renameSession: async (id, title) => {
      set({ busy: true, error: null })
      try {
        const updated = await updateSession(id, title)
        stampTitle(id)
        set((state) => ({
          busy: false,
          sessions: state.sessions.map((session) => (session.id === id ? updated : session))
        }))
      } catch (err) {
        set({
          busy: false,
          error: noteError(deps, errorText(err, 'Failed to rename chat session'), id)
        })
      }
    },

    removeSession: async (id) => {
      set({ busy: true, error: null })
      try {
        await deleteSession(id)
      } catch (err) {
        set({
          busy: false,
          error: noteError(deps, errorText(err, 'Failed to delete chat session'), id)
        })
        return
      }

      set({ busy: false })
      await get().forgetSession(id)
    },

    forgetSession: async (id) => {
      const epoch = bumpHydrate()
      const next = get().sessions.filter((session) => session.id !== id)
      const wasActive = get().activeSessionId === id && !get().draftSelected
      set((state) => ({
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
        const transcript = await readTranscript(fallback.id)
        if (epoch !== hydrateEpoch) {
          return
        }
        if (transcript) {
          applyTranscript(
            set,
            fallback.id,
            transcript.messages,
            transcript.session,
            transcript.seenAt
          )
        }
      } catch (err) {
        if (epoch !== hydrateEpoch) {
          return
        }
        set({ error: noteError(deps, errorText(err, 'Failed to load chat session'), fallback.id) })
      }
    },

    upsertMessage: (sessionId, message) => {
      stampMessage(sessionId, message.id)
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

    noteRunning: (sessionId, running) => {
      set((state) => {
        const phase = state.phaseBySession[sessionId]
        const nextPhase = running ? (phase === 'responding' ? 'responding' : 'thinking') : 'idle'
        const phaseSame = running ? phase === nextPhase : phase === undefined || phase === 'idle'
        const session = state.sessions.find((item) => item.id === sessionId)
        const flagSame = session === undefined || session.has_pending_agent === running
        if (phaseSame && flagSame) {
          return state
        }
        if (session && !flagSame) {
          stampPending(sessionId)
        }
        return {
          phaseBySession: phaseSame
            ? state.phaseBySession
            : { ...state.phaseBySession, [sessionId]: nextPhase },
          sessions:
            session && !flagSame
              ? state.sessions.map((item) =>
                  item.id === sessionId ? { ...item, has_pending_agent: running } : item
                )
              : state.sessions
        }
      })
    },

    setPhase: (sessionId, phase) => {
      if (phase === 'thinking' || phase === 'responding') {
        stampPending(sessionId)
      }
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
      stampPending(sessionId)
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
      if (failedMessage) {
        noteError(deps, failedMessage, sessionId)
      }
      if (!stopping) {
        set((state) => ({
          phaseBySession: { ...state.phaseBySession, [sessionId]: 'idle' },
          error: failedMessage ?? state.error
        }))
      } else if (failedMessage) {
        set({ error: failedMessage })
      }
      try {
        const transcript = await readTranscript(sessionId)
        if (tick !== stopTick || epoch !== hydrateEpoch || !transcript) {
          return
        }
        // The actor clears `running` after it emits `turn_finished`, so this
        // session can still report `has_pending_agent`. Leave the phase idle.
        // A `turn_started` during the refetch sets Thinking and is kept.
        applyTranscript(
          set,
          sessionId,
          transcript.messages,
          transcript.session,
          transcript.seenAt,
          'preserve'
        )
        get().bumpReview(sessionId)
        if (failedMessage) {
          set({ error: failedMessage })
        }
      } catch (err) {
        if (epoch !== hydrateEpoch) {
          return
        }
        set({ error: noteError(deps, errorText(err, 'Failed to load chat session'), sessionId) })
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
        const transcript = await readTranscript(sessionId)
        if (!transcript || epoch !== hydrateEpoch || get().stoppingSessionId === sessionId) {
          return
        }
        const current = get().phaseBySession[sessionId]
        if (current !== 'thinking' && current !== 'responding') {
          return
        }
        applyTranscript(set, sessionId, transcript.messages, transcript.session, transcript.seenAt)
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
      const seenAt = revisionNow()
      const generation = startFetch(sessionKey(sessionId))
      try {
        const session = await getSession(sessionId)
        if (epoch !== hydrateEpoch || !fetchStillCurrent(sessionKey(sessionId), generation)) {
          return
        }
        set((state) => ({
          sessions: replaceSession(state.sessions, session, seenAt)
        }))
      } catch (err) {
        if (epoch !== hydrateEpoch || !fetchStillCurrent(sessionKey(sessionId), generation)) {
          return
        }
        if (err instanceof ApiError && err.status === 404) {
          return
        }
        set({ error: noteError(deps, errorText(err, 'Failed to load chat session'), sessionId) })
      }
    },

    noteTurnDisplay: (sessionId, turnDisplay) => {
      turnDisplayRevision.set(sessionId, bumpRevision())
      set((state) => ({
        sessions: state.sessions.map((item) =>
          item.id === sessionId
            ? {
                ...item,
                turn_display: turnDisplay,
                has_pending_agent:
                  turnDisplay === 'awaiting_approval' ? false : item.has_pending_agent
              }
            : item
        )
      }))
    },

    renameSessionLocal: (sessionId, title) => {
      stampTitle(sessionId)
      set((state) => ({
        sessions: state.sessions.map((item) => (item.id === sessionId ? { ...item, title } : item))
      }))
    },

    hydrateFromStream: async () => {
      const epoch = hydrateEpoch
      const { draftSelected, activeSessionId } = get()
      const workspaceId = deps.activeWorkspaceId()
      if (!workspaceId) {
        return
      }
      const seenAt = revisionNow()
      const listGeneration = startFetch(sessionsListKey(workspaceId))
      try {
        const next = await listSessions(workspaceId)
        if (
          epoch !== hydrateEpoch ||
          !fetchStillCurrent(sessionsListKey(workspaceId), listGeneration)
        ) {
          return
        }
        set((state) => ({ sessions: mergeSessionList(state.sessions, next, seenAt) }))
        if (draftSelected || !activeSessionId) {
          return
        }
        if (!get().sessions.some((session) => session.id === activeSessionId)) {
          const fallback = next[0]
          if (!fallback) {
            set({ draftSelected: true, activeSessionId: null })
            return
          }
          set({ activeSessionId: fallback.id, draftSelected: false })
          const transcript = await readTranscript(fallback.id)
          if (epoch !== hydrateEpoch || !transcript) {
            return
          }
          applyTranscript(
            set,
            fallback.id,
            transcript.messages,
            transcript.session,
            transcript.seenAt
          )
          get().bumpReview(fallback.id)
          return
        }
        const transcript = await readTranscript(activeSessionId)
        if (epoch !== hydrateEpoch || !transcript) {
          return
        }
        applyTranscript(
          set,
          activeSessionId,
          transcript.messages,
          transcript.session,
          transcript.seenAt
        )
        get().bumpReview(activeSessionId)
      } catch (err) {
        if (
          epoch !== hydrateEpoch ||
          !fetchStillCurrent(sessionsListKey(workspaceId), listGeneration)
        ) {
          return
        }
        set({ error: noteError(deps, errorText(err, 'Failed to load chat sessions')) })
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
  }
}

const chatHost = mountStore<ChatState>((set, get) =>
  createChatState(set, get, {
    reportError: (message, sessionId) => errors.report(message, sessionId ?? null),
    activeWorkspaceId: () => workspaces.activeWorkspaceId
  })
)

export const chat = chatHost.state

export function patchChat(partial: Partial<ChatState>): void {
  chatHost.set(partial)
}
