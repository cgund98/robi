import { useIsRouting, useLocation, useNavigate } from '@solidjs/router'
import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { createEffect, createMemo, createSignal, onCleanup, onMount, Show } from 'solid-js'

import { listModels, type CatalogModel } from '../../api/models'
import { sessionDisplayTitle, type AgentMode, type ChatSession } from '../../api/sessions'
import { getSettings, SETTING_KEYS } from '../../api/settings'
import { showSurface } from '../../app/surfaces'
import { planBuildInstruction, type PlanView } from '../../features/chat/toolCallView'
import styles from './AppLayout.module.css'
import { sessionMark } from './recents'
import { sessionMode, type AgentPhase } from '../../state/chatStore'
import { useAgentEvents } from '../../features/chat/useAgentEvents'
import { chat } from '../../state/chatStore'
import { workspaces } from '../../state/workspaceStore'
import { ChatHeader } from '../../features/chat/ChatHeader'
import { ChatPanel } from '../../features/chat/ChatPanel'
import { Composer } from '../../features/chat/Composer'
import { DeleteSessionDialog } from '../../features/chat/DeleteSessionDialog'
import { EmptyGreeting } from '../../features/chat/EmptyGreeting'
import { PlanPage } from '../../features/chat/PlanPage'
import { RenameSessionDialog } from '../../features/chat/RenameSessionDialog'
import { ErrorNotices } from './ErrorNotices'
import { Sidebar } from './Sidebar'
import { TopBar } from './TopBar'

async function buildPlan(path: string) {
  await chat.setModeChoice('agent')
  const mode =
    chat.draftSelected || chat.activeSessionId === null
      ? chat.draftMode
      : sessionMode(chat.sessions.find((session) => session.id === chat.activeSessionId))
  if (mode !== 'agent') {
    return
  }
  await chat.sendInstruction(planBuildInstruction(path))
}

type ModeDefault = { model: string | null; effort: string | null }

const MODE_SETTING_KEYS: Record<AgentMode, { model: string; effort: string }> = {
  ask: { model: SETTING_KEYS.modelAsk, effort: SETTING_KEYS.reasoningEffortAsk },
  plan: { model: SETTING_KEYS.modelPlan, effort: SETTING_KEYS.reasoningEffortPlan },
  agent: { model: SETTING_KEYS.modelAgent, effort: SETTING_KEYS.reasoningEffortAgent }
}

/**
 * Chat shell: the bar (or, in the legacy layout, the sidebar and header), and
 * the welcome, thread, or plan pane.
 */
export function ChatChrome() {
  const navigate = useNavigate()
  const location = useLocation()
  const isRouting = useIsRouting()
  const [renameId, setRenameId] = createSignal<string | null>(null)
  const [renameError, setRenameError] = createSignal<string | null>(null)
  const [deleteId, setDeleteId] = createSignal<string | null>(null)
  const [deleteError, setDeleteError] = createSignal<string | null>(null)
  const [plan, setPlan] = createSignal<{
    view: PlanView
    sessionId: string | null
  } | null>(null)
  const [models, setModels] = createSignal<CatalogModel[]>([])
  const [fallbackModelId, setFallbackModelId] = createSignal('glm-5.3')
  const [fallbackEffort, setFallbackEffort] = createSignal<string | null>(null)
  const [modeDefaults, setModeDefaults] = createSignal<Record<AgentMode, ModeDefault>>({
    ask: { model: null, effort: null },
    plan: { model: null, effort: null },
    agent: { model: null, effort: null }
  })

  useAgentEvents()

  onMount(() => {
    void workspaces.loadWorkspaces()
    let cancelled = false
    void listModels()
      .then((next) => {
        if (!cancelled) {
          setModels(next)
        }
      })
      .catch(() => {})
    const modes: AgentMode[] = ['ask', 'plan', 'agent']
    void getSettings([
      SETTING_KEYS.model,
      SETTING_KEYS.reasoningEffort,
      ...modes.flatMap((mode) => {
        const keys = MODE_SETTING_KEYS[mode]
        return [keys.model, keys.effort]
      })
    ])
      .then((settings) => {
        if (cancelled) {
          return
        }
        const [model, effort, ...modeSettings] = settings
        if (model?.value) {
          setFallbackModelId(model.value)
        }
        setFallbackEffort(effort?.value ?? null)
        const entries = modes.map((mode, index) => {
          const modelSetting = modeSettings[index * 2]
          const effortSetting = modeSettings[index * 2 + 1]
          return [
            mode,
            { model: modelSetting?.value ?? null, effort: effortSetting?.value ?? null }
          ] as const
        })
        setModeDefaults(Object.fromEntries(entries) as Record<AgentMode, ModeDefault>)
      })
      .catch(() => {})
    if (isTauri()) {
      let unlisten: (() => void) | undefined
      void listen<string>('approval-notice-open', (event) => {
        if (event.payload) {
          navigate(`/sessions/${event.payload}`)
          void chat.selectSession(event.payload)
        }
      }).then((stop) => {
        unlisten = stop
      })
      onCleanup(() => unlisten?.())
    }
    onCleanup(() => {
      cancelled = true
    })
  })

  let previousWorkspace: string | null | undefined = undefined
  createEffect(() => {
    if (!workspaces.loaded) {
      return
    }
    const switched =
      previousWorkspace !== undefined && previousWorkspace !== workspaces.activeWorkspaceId
    previousWorkspace = workspaces.activeWorkspaceId
    void chat.loadSessions(switched ? { draft: true } : undefined)
  })

  createEffect(() => {
    if (workspaces.loaded && workspaces.workspaces.length === 0) {
      navigate('/workspaces', { replace: true })
    }
  })

  const chatSessionId = () => /^\/sessions\/([^/]+)$/.exec(location.pathname)?.[1] ?? null

  createEffect(() => {
    const id = chatSessionId()
    if (isRouting() || chat.loading || !id) {
      return
    }
    if (!chat.sessions.some((session) => session.id === id)) {
      return
    }
    if (chat.activeSessionId !== id || chat.draftSelected) {
      void chat.selectSession(id)
    }
  })

  createEffect(() => {
    const current = plan()
    if (current && (chat.draftSelected || current.sessionId !== chat.activeSessionId)) {
      setPlan(null)
    }
  })

  const activeSession = createMemo(() =>
    !chat.draftSelected && chat.activeSessionId
      ? (chat.sessions.find((session) => session.id === chat.activeSessionId) ?? null)
      : null
  )
  const sessionTitle = createMemo(() => {
    if (chat.loading && !activeSession() && !chat.draftSelected) {
      return 'Loading…'
    }
    return sessionDisplayTitle(activeSession())
  })
  const runningIds = createMemo(() =>
    chat.sessions
      .filter((session) => sessionMark(session, chat.phaseBySession[session.id]) === 'running')
      .map((session) => session.id)
  )
  const awaitingIds = createMemo(() =>
    chat.sessions
      .filter((session) => sessionMark(session, chat.phaseBySession[session.id]) === 'awaiting')
      .map((session) => session.id)
  )
  const phase = createMemo((): AgentPhase => {
    if (chat.draftSelected || !chat.activeSessionId) {
      return 'idle'
    }
    return chat.phaseBySession[chat.activeSessionId!] ?? 'idle'
  })
  const messages = createMemo(() =>
    chat.activeSessionId && !chat.draftSelected
      ? (chat.messagesBySession[chat.activeSessionId!] ?? [])
      : []
  )
  const echo = createMemo(() =>
    chat.activeSessionId &&
    !chat.draftSelected &&
    chat.pendingEcho?.sessionId === chat.activeSessionId
      ? chat.pendingEcho!.text
      : null
  )
  const echoFiles = createMemo(() =>
    chat.activeSessionId &&
    !chat.draftSelected &&
    chat.pendingEcho?.sessionId === chat.activeSessionId
      ? chat.pendingEcho!.files
      : []
  )
  const mode = createMemo((): AgentMode =>
    chat.draftSelected || !activeSession() ? chat.draftMode : sessionMode(activeSession())
  )
  const defaultModelId = () => modeDefaults()[mode()].model ?? fallbackModelId()
  const defaultEffort = () => modeDefaults()[mode()].effort ?? fallbackEffort()
  const modelId = () =>
    chat.draftSelected || !activeSession() ? chat.draftModel : sessionModel(activeSession(), mode())
  const effort = () =>
    chat.draftSelected || !activeSession()
      ? chat.draftEffort
      : sessionEffort(activeSession(), mode())
  const agentRunning = () => phase() !== 'idle'
  const composerLocked = () => chat.loading || chat.busy || stopping()
  const composerDraftKey = () =>
    chat.draftSelected || !chat.activeSessionId ? 'draft' : chat.activeSessionId!
  const stopping = () =>
    !chat.draftSelected &&
    chat.activeSessionId !== null &&
    chat.stoppingSessionId === chat.activeSessionId
  const fresh = () => messages().length === 0 && echo() === null
  const renameTarget = () => chat.sessions.find((session) => session.id === renameId()) ?? null
  const deleteTarget = () => chat.sessions.find((session) => session.id === deleteId()) ?? null

  const openSession = (id: string) => {
    setPlan(null)
    // Navigate first: the route reaches `location` on a later microtask, so the
    // reconciling effect below must see `isRouting()` already true and bail.
    // Writing the store first would let that effect re-select the old route.
    if (location.pathname !== `/sessions/${id}`) {
      navigate(`/sessions/${id}`)
    }
    void chat.selectSession(id)
  }
  const openDraft = () => {
    setPlan(null)
    // See openSession: navigate before the store write so the reconciling effect
    // does not read a stale `/sessions/:id` route and undo the draft selection.
    if (location.pathname !== '/') {
      navigate('/')
    }
    chat.selectDraft()
  }

  const chatProps = () => ({
    messages: messages(),
    sessionId: chat.activeSessionId,
    echo: echo(),
    echoFiles: echoFiles(),
    phase: phase(),
    mode: mode(),
    deciding: chat.busy,
    buildDisabled: chat.loading || chat.busy || agentRunning() || stopping(),
    onDecide: (callId: string, decision: 'approve' | 'reject') => {
      const id = chat.activeSessionId
      if (id) {
        void chat.decideCall(id, callId, decision)
      }
    },
    onBuild: (path: string) => {
      void buildPlan(path)
    },
    onViewPlan: (next: PlanView) => setPlan({ view: next, sessionId: chat.activeSessionId }),
    disabled: composerLocked(),
    pending: chat.busy,
    running: agentRunning(),
    stopping: stopping(),
    onStop: () => void chat.stopAgent(),
    onSubmit: chat.sendInstruction,
    models: models(),
    modelId: modelId(),
    effort: effort(),
    defaultModelId: defaultModelId(),
    defaultEffort: defaultEffort(),
    onModeChange: (next: AgentMode) => void chat.setModeChoice(next),
    onModelChange: (model: string | null) => void chat.setModelChoice(model),
    onEffortChange: (next: string | null) => void chat.setEffortChoice(next),
    draftKey: composerDraftKey(),
    pendingText: echo(),
    workspaceId: workspaces.activeWorkspaceId,
    onCompact: () => void chat.compactAgent(),
    compacting: chat.compactingSessionId === chat.activeSessionId
  })

  return (
    <div class={styles.shell}>
      <Show when={!showSurface('topBarShell')}>
        <Sidebar
          sessions={chat.sessions}
          activeSessionId={chat.draftSelected ? '' : (activeSession()?.id ?? '')}
          draftSelected={chat.draftSelected}
          docsSelected={false}
          disabled={chat.loading}
          runningSessionIds={new Set(runningIds())}
          awaitingSessionIds={new Set(awaitingIds())}
          onSelectSession={openSession}
          onNewSession={openDraft}
          onRenameSession={(id) => {
            setRenameError(null)
            setRenameId(id)
          }}
          onDeleteSession={(id) => {
            setDeleteError(null)
            setDeleteId(id)
          }}
        />
      </Show>
      <div class={styles.main}>
        <Show
          when={showSurface('topBarShell')}
          fallback={<ChatHeader sessionTitle={sessionTitle()} />}
        >
          <TopBar
            sessionTitle={sessionTitle()}
            sessions={chat.sessions}
            activeSessionId={chat.draftSelected ? '' : (activeSession()?.id ?? '')}
            draftSelected={chat.draftSelected}
            phaseBySession={chat.phaseBySession}
            disabled={chat.loading}
            onSelectSession={openSession}
            onNewSession={openDraft}
            onRenameSession={(id) => {
              setRenameError(null)
              setRenameId(id)
            }}
            onDeleteSession={(id) => {
              setDeleteError(null)
              setDeleteId(id)
            }}
          />
        </Show>
        <ErrorNotices />
        <Show when={plan()}>
          <PlanPage
            plan={plan()!.view}
            buildDisabled={
              chat.loading ||
              chat.busy ||
              agentRunning() ||
              stopping() ||
              plan()!.view.path.length === 0
            }
            onBack={() => setPlan(null)}
            onBuild={() => {
              const path = plan()!.view.path
              setPlan(null)
              void buildPlan(path)
            }}
          />
        </Show>
        <Show
          when={!plan() && chat.transcriptLoading && messages().length === 0 && echo() === null}
        >
          <div class={styles.loadingTranscript} role="status">
            <span class={styles.spinner} aria-hidden="true" />
            Loading conversation
          </div>
        </Show>
        <Show
          when={
            !plan() &&
            !(chat.transcriptLoading && messages().length === 0 && echo() === null) &&
            fresh()
          }
        >
          <div class={styles.welcome}>
            <EmptyGreeting />
            <Composer placement="welcome" {...chatProps()} />
          </div>
        </Show>
        <Show
          when={
            !plan() &&
            !(chat.transcriptLoading && messages().length === 0 && echo() === null) &&
            !fresh()
          }
        >
          <ChatPanel {...chatProps()} />
        </Show>
      </div>
      <Show when={renameTarget()}>
        {(target) => (
          <RenameSessionDialog
            open
            initialTitle={target().title ?? ''}
            busy={chat.busy}
            error={renameError()}
            onCancel={() => {
              setRenameError(null)
              setRenameId(null)
            }}
            onSave={(title) => {
              void chat.renameSession(target().id, title).then(() => {
                const message = chat.error
                if (message) {
                  setRenameError(message)
                  return
                }
                setRenameId(null)
              })
            }}
          />
        )}
      </Show>
      <Show when={deleteTarget()}>
        {(target) => (
          <DeleteSessionDialog
            open
            title={sessionDisplayTitle(target())}
            running={runningIds().includes(target().id)}
            busy={chat.busy}
            error={deleteError()}
            onCancel={() => {
              setDeleteError(null)
              setDeleteId(null)
            }}
            onDelete={() => {
              void chat.removeSession(target().id).then(() => {
                const message = chat.error
                if (message) {
                  setDeleteError(message)
                  return
                }
                setDeleteId(null)
              })
            }}
          />
        )}
      </Show>
    </div>
  )
}

function sessionModel(session: ChatSession | null, mode: AgentMode): string | null {
  return session?.model_config[mode]?.model ?? null
}

function sessionEffort(session: ChatSession | null, mode: AgentMode): string | null {
  return session?.model_config[mode]?.reasoning_effort ?? null
}
