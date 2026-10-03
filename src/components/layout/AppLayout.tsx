import { useEffect, useRef, useState } from 'react'
import { useMatch, useNavigate } from 'react-router-dom'

import { listModels, type CatalogModel } from '../../api/models'
import { sessionDisplayTitle, type AgentMode, type ChatSession } from '../../api/sessions'
import { getSetting, SETTING_KEYS } from '../../api/settings'
import { useApprovalNoticeOpen } from '../../app/approvalNotice'
import { useAgentEventsSSE } from '../../app/useAgentEventsSSE'
import { sessionMode, useChatStore, type AgentPhase } from '../../state/chatStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import { ChatHeader } from '../chat/ChatHeader'
import { Composer } from '../chat/Composer'
import { EditReviewStrip } from '../chat/EditReviewStrip'
import { EmptyGreeting } from '../chat/EmptyGreeting'
import { RenameSessionDialog } from '../chat/RenameSessionDialog'
import { PlanPage } from '../chat/PlanPage'
import { planBuildInstruction, type PlanView } from '../chat/toolCallView'
import { Transcript } from '../chat/Transcript'
import { ReviewScreen } from '../review/ReviewScreen'
import { Sidebar } from './Sidebar'
import styles from './AppLayout.module.css'

async function buildPlan(path: string) {
  const store = useChatStore.getState()
  await store.setModeChoice('agent')
  const next = useChatStore.getState()
  const mode =
    next.draftSelected || next.activeSessionId === null
      ? next.draftMode
      : sessionMode(next.sessions.find((session) => session.id === next.activeSessionId))
  if (mode !== 'agent') {
    return
  }
  await next.sendInstruction(planBuildInstruction(path))
}

function runningSessionIds(
  sessions: ChatSession[],
  phaseBySession: Record<string, AgentPhase>
): ReadonlySet<string> {
  const running = new Set<string>()
  for (const session of sessions) {
    const phase = phaseBySession[session.id]
    if (session.has_pending_agent || (phase !== undefined && phase !== 'idle')) {
      running.add(session.id)
    }
  }
  return running
}

export function AppLayout() {
  const sessions = useChatStore((state) => state.sessions)
  const activeSessionId = useChatStore((state) => state.activeSessionId)
  const draftSelected = useChatStore((state) => state.draftSelected)
  const messagesBySession = useChatStore((state) => state.messagesBySession)
  const phaseBySession = useChatStore((state) => state.phaseBySession)
  const pendingEcho = useChatStore((state) => state.pendingEcho)
  const error = useChatStore((state) => state.error)
  const loading = useChatStore((state) => state.loading)
  const busy = useChatStore((state) => state.busy)
  const loadSessions = useChatStore((state) => state.loadSessions)
  const workspacesLoaded = useWorkspaceStore((state) => state.loaded)
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const workspaceError = useWorkspaceStore((state) => state.error)
  const loadWorkspaces = useWorkspaceStore((state) => state.loadWorkspaces)
  const workspaceCount = useWorkspaceStore((state) => state.workspaces.length)
  const previousWorkspace = useRef<string | null | undefined>(undefined)
  const [renameId, setRenameId] = useState<string | null>(null)
  const [renameError, setRenameError] = useState<string | null>(null)
  const [plan, setPlan] = useState<{ view: PlanView; sessionId: string | null } | null>(null)
  const navigate = useNavigate()
  const reviewMatch = useMatch('/sessions/:sessionId/review')
  const reviewSessionId = reviewMatch?.params.sessionId ?? null
  if (plan && (draftSelected || reviewSessionId !== null || plan.sessionId !== activeSessionId)) {
    setPlan(null)
  }
  const selectSession = useChatStore((state) => state.selectSession)
  const selectDraft = useChatStore((state) => state.selectDraft)
  const draftMode = useChatStore((state) => state.draftMode)
  const draftModel = useChatStore((state) => state.draftModel)
  const draftEffort = useChatStore((state) => state.draftEffort)
  const setModeChoice = useChatStore((state) => state.setModeChoice)
  const setModelChoice = useChatStore((state) => state.setModelChoice)
  const setEffortChoice = useChatStore((state) => state.setEffortChoice)
  const sendInstruction = useChatStore((state) => state.sendInstruction)
  const stopAgent = useChatStore((state) => state.stopAgent)
  const stoppingSessionId = useChatStore((state) => state.stoppingSessionId)
  const { models, fallbackModelId, fallbackEffort, modeDefaults } = useModelDefaults()
  const decideCall = useChatStore((state) => state.decideCall)
  const renameSession = useChatStore((state) => state.renameSession)
  const removeSession = useChatStore((state) => state.removeSession)

  useAgentEventsSSE()
  useApprovalNoticeOpen()

  useEffect(() => {
    void loadWorkspaces()
  }, [loadWorkspaces])

  useEffect(() => {
    if (workspacesLoaded && workspaceCount === 0) {
      navigate('/workspaces', { replace: true })
    }
  }, [workspacesLoaded, workspaceCount, navigate])

  useEffect(() => {
    if (!reviewSessionId) {
      return
    }
    void selectSession(reviewSessionId)
  }, [reviewSessionId, selectSession])

  useEffect(() => {
    if (!workspacesLoaded) {
      return
    }
    const switched =
      previousWorkspace.current !== undefined && previousWorkspace.current !== activeWorkspaceId
    previousWorkspace.current = activeWorkspaceId
    void loadSessions(switched ? { draft: true } : undefined)
  }, [workspacesLoaded, activeWorkspaceId, loadSessions])

  const activeSession =
    !draftSelected && activeSessionId
      ? (sessions.find((session) => session.id === activeSessionId) ?? null)
      : null
  const phase: AgentPhase =
    activeSessionId && !draftSelected ? (phaseBySession[activeSessionId] ?? 'idle') : 'idle'
  const messages =
    activeSessionId && !draftSelected ? (messagesBySession[activeSessionId] ?? []) : []
  const echo =
    activeSessionId && !draftSelected && pendingEcho?.sessionId === activeSessionId
      ? pendingEcho.text
      : null
  const agentRunning = phase !== 'idle'
  const composerLocked = loading || busy || agentRunning
  const stopping =
    !draftSelected && activeSessionId !== null && stoppingSessionId === activeSessionId
  const fresh = messages.length === 0 && echo === null
  const threadRef = useRef<HTMLDivElement>(null)
  const dockRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const dock = dockRef.current
    const thread = threadRef.current
    if (!dock || !thread) {
      return
    }
    const apply = () => {
      thread.style.setProperty('--dock-height', `${dock.offsetHeight}px`)
      const composer = dock.lastElementChild
      if (composer) {
        thread.style.setProperty('--composer-height', `${composer.offsetHeight}px`)
      }
    }
    apply()
    const observer = new ResizeObserver(apply)
    observer.observe(dock)
    return () => observer.disconnect()
  }, [fresh, reviewSessionId, activeSessionId, plan])

  const renameTarget = renameId
    ? (sessions.find((session) => session.id === renameId) ?? null)
    : null

  async function handleSaveTitle(title: string) {
    if (!renameId) {
      return
    }
    await renameSession(renameId, title)
    const message = useChatStore.getState().error
    if (message) {
      setRenameError(message)
      return
    }
    setRenameId(null)
  }

  async function handleDeleteSession(id: string) {
    const current = sessions.find((session) => session.id === id)
    const label = sessionDisplayTitle(current)
    if (!window.confirm(`Delete “${label}”? This cannot be undone.`)) {
      return
    }
    await removeSession(id)
  }

  const sessionTitle =
    loading && !activeSession && !draftSelected ? 'Loading…' : sessionDisplayTitle(activeSession)
  const mode: AgentMode = draftSelected || !activeSession ? draftMode : sessionMode(activeSession)
  const modeDefault = modeDefaults[mode]
  const defaultModelId = modeDefault.model ?? fallbackModelId
  const defaultEffort = modeDefault.effort ?? fallbackEffort
  const modelId = draftSelected || !activeSession ? draftModel : sessionModel(activeSession, mode)
  const effort = draftSelected || !activeSession ? draftEffort : sessionEffort(activeSession, mode)

  return (
    <div className={styles.shell}>
      <Sidebar
        sessions={sessions}
        activeSessionId={draftSelected ? '' : (activeSession?.id ?? '')}
        draftSelected={draftSelected}
        disabled={loading || busy}
        runningSessionIds={runningSessionIds(sessions, phaseBySession)}
        onSelectSession={(id) => {
          setPlan(null)
          if (reviewSessionId) {
            navigate('/')
          }
          void selectSession(id)
        }}
        onNewSession={() => {
          setPlan(null)
          if (reviewSessionId) {
            navigate('/')
          }
          selectDraft()
        }}
        onRenameSession={(id) => {
          setRenameError(null)
          setRenameId(id)
        }}
        onDeleteSession={(id) => void handleDeleteSession(id)}
      />
      <div className={styles.main}>
        {workspaceError || error ? (
          <div className={styles.banner} role="alert">
            <span>{workspaceError ?? error}</span>
            <button
              type="button"
              className={styles.bannerRetry}
              onClick={() => {
                void loadWorkspaces()
                void loadSessions()
              }}
            >
              Retry
            </button>
          </div>
        ) : null}
        {reviewSessionId ? (
          <ReviewScreen key={reviewSessionId} sessionId={reviewSessionId} />
        ) : plan ? (
          <PlanPage
            plan={plan.view}
            buildDisabled={composerLocked || plan.view.path.length === 0}
            onBack={() => setPlan(null)}
            onBuild={() => {
              const path = plan.view.path
              setPlan(null)
              void buildPlan(path)
            }}
          />
        ) : fresh ? (
          <div className={styles.welcome}>
            <EmptyGreeting />
            <Composer
              placement="welcome"
              disabled={composerLocked}
              running={agentRunning}
              stopping={stopping}
              onStop={() => void stopAgent()}
              onSubmit={sendInstruction}
              models={models}
              mode={mode}
              modelId={modelId}
              effort={effort}
              defaultModelId={defaultModelId}
              defaultEffort={defaultEffort}
              onModeChange={(next) => void setModeChoice(next)}
              onModelChange={(model) => void setModelChoice(model)}
              onEffortChange={(next) => void setEffortChoice(next)}
              messages={messages}
              pendingText={echo}
            />
          </div>
        ) : (
          <div className={styles.chat}>
            <ChatHeader sessionTitle={sessionTitle} />
            <div className={styles.thread} ref={threadRef}>
              <Transcript
                messages={messages}
                echo={echo}
                phase={phase}
                mode={mode}
                deciding={busy}
                buildDisabled={composerLocked}
                onDecide={(callId, decision) => {
                  if (activeSessionId) {
                    void decideCall(activeSessionId, callId, decision)
                  }
                }}
                onBuild={(path) => {
                  void buildPlan(path)
                }}
                onViewPlan={(next) => {
                  setPlan({ view: next, sessionId: activeSessionId })
                }}
              />
              <div className={styles.dock} ref={dockRef}>
                {activeSessionId ? (
                  <EditReviewStrip key={activeSessionId} sessionId={activeSessionId} />
                ) : null}
                <Composer
                  disabled={composerLocked}
                  running={agentRunning}
                  stopping={stopping}
                  onStop={() => void stopAgent()}
                  onSubmit={sendInstruction}
                  models={models}
                  mode={mode}
                  modelId={modelId}
                  effort={effort}
                  defaultModelId={defaultModelId}
                  defaultEffort={defaultEffort}
                  onModeChange={(next) => void setModeChoice(next)}
                  onModelChange={(model) => void setModelChoice(model)}
                  onEffortChange={(next) => void setEffortChoice(next)}
                  messages={messages}
                  pendingText={echo}
                />
              </div>
            </div>
          </div>
        )}
      </div>
      {renameTarget ? (
        <RenameSessionDialog
          key={renameTarget.id}
          open
          initialTitle={renameTarget.title ?? ''}
          busy={busy}
          error={renameError}
          onCancel={() => {
            setRenameError(null)
            setRenameId(null)
          }}
          onSave={(title) => void handleSaveTitle(title)}
        />
      ) : null}
    </div>
  )
}

function sessionModel(session: ChatSession, mode: AgentMode): string | null {
  return session.model_config[mode]?.model ?? null
}

function sessionEffort(session: ChatSession, mode: AgentMode): string | null {
  return session.model_config[mode]?.reasoning_effort ?? null
}

const MODE_SETTING_KEYS: Record<AgentMode, { model: string; effort: string }> = {
  ask: { model: SETTING_KEYS.modelAsk, effort: SETTING_KEYS.reasoningEffortAsk },
  plan: { model: SETTING_KEYS.modelPlan, effort: SETTING_KEYS.reasoningEffortPlan },
  agent: { model: SETTING_KEYS.modelAgent, effort: SETTING_KEYS.reasoningEffortAgent }
}

type ModeDefault = { model: string | null; effort: string | null }

function useModelDefaults(): {
  models: CatalogModel[]
  fallbackModelId: string
  fallbackEffort: string | null
  modeDefaults: Record<AgentMode, ModeDefault>
} {
  const [models, setModels] = useState<CatalogModel[]>([])
  const [fallbackModelId, setFallbackModelId] = useState('glm-5.3')
  const [fallbackEffort, setFallbackEffort] = useState<string | null>(null)
  const [modeDefaults, setModeDefaults] = useState<Record<AgentMode, ModeDefault>>({
    ask: { model: null, effort: null },
    plan: { model: null, effort: null },
    agent: { model: null, effort: null }
  })

  useEffect(() => {
    let cancelled = false
    void listModels()
      .then((next) => {
        if (!cancelled) {
          setModels(next)
        }
      })
      .catch(() => {})
    void getSetting(SETTING_KEYS.model)
      .then((setting) => {
        if (!cancelled && setting.value) {
          setFallbackModelId(setting.value)
        }
      })
      .catch(() => {})
    void getSetting(SETTING_KEYS.reasoningEffort)
      .then((setting) => {
        if (!cancelled) {
          setFallbackEffort(setting.value)
        }
      })
      .catch(() => {})
    const modes: AgentMode[] = ['ask', 'plan', 'agent']
    void Promise.all(
      modes.map(async (mode) => {
        const keys = MODE_SETTING_KEYS[mode]
        const [model, effort] = await Promise.all([getSetting(keys.model), getSetting(keys.effort)])
        return [mode, { model: model.value, effort: effort.value }] as const
      })
    )
      .then((entries) => {
        if (!cancelled) {
          setModeDefaults(Object.fromEntries(entries) as Record<AgentMode, ModeDefault>)
        }
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [])

  return { models, fallbackModelId, fallbackEffort, modeDefaults }
}
