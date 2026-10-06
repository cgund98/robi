import { useCallback, useEffect, useRef, useState } from 'react'
import { useMatch, useNavigate } from 'react-router-dom'
import { useShallow } from 'zustand/react/shallow'

import { listModels, type CatalogModel } from '../../api/models'
import { sessionDisplayTitle, type AgentMode, type ChatSession } from '../../api/sessions'
import { getSettings, SETTING_KEYS } from '../../api/settings'
import { useApprovalNoticeOpen } from '../../app/approvalNotice'
import { useAgentEventsSSE } from '../../app/useAgentEventsSSE'
import { requestComposerAttachment } from '../../state/composerAttachments'
import { sessionMode, useChatStore, type AgentPhase } from '../../state/chatStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import { ChatHeader } from '../chat/ChatHeader'
import { ChatPanel, type ChatPanelProps } from '../chat/ChatPanel'
import { ChatTray } from '../chat/ChatTray'
import { Composer } from '../chat/Composer'
import { EmptyGreeting } from '../chat/EmptyGreeting'
import { RenameSessionDialog } from '../chat/RenameSessionDialog'
import { DeleteSessionDialog } from '../chat/DeleteSessionDialog'
import { PlanPage } from '../chat/PlanPage'
import { planBuildInstruction, type PlanView } from '../chat/toolCallView'
import type { FileAttachment } from '../chat/textAttachments'
import { DocsScreen } from '../docs/DocsScreen'
import { ReviewScreen } from '../review/ReviewScreen'
import { ErrorNotices } from './ErrorNotices'
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

export function AppLayout() {
  const sessions = useChatStore((state) => state.sessions)
  const activeSessionId = useChatStore((state) => state.activeSessionId)
  const draftSelected = useChatStore((state) => state.draftSelected)
  const messagesBySession = useChatStore((state) => state.messagesBySession)
  const runningIds = useChatStore(
    useShallow((state) => {
      const ids: string[] = []
      for (const session of state.sessions) {
        const sessionPhase = state.phaseBySession[session.id]
        if (session.turn_display === 'awaiting_approval') {
          continue
        }
        if (
          session.has_pending_agent ||
          session.turn_display === 'pending' ||
          (sessionPhase !== undefined && sessionPhase !== 'idle')
        ) {
          ids.push(session.id)
        }
      }
      return ids
    })
  )
  const awaitingIds = useChatStore(
    useShallow((state) =>
      state.sessions
        .filter((session) => session.turn_display === 'awaiting_approval')
        .map((session) => session.id)
    )
  )
  const phase = useChatStore((state): AgentPhase => {
    if (state.draftSelected || !state.activeSessionId) {
      return 'idle'
    }
    return state.phaseBySession[state.activeSessionId] ?? 'idle'
  })
  const pendingEcho = useChatStore((state) => state.pendingEcho)
  const loading = useChatStore((state) => state.loading)
  const transcriptLoading = useChatStore((state) => state.transcriptLoading)
  const busy = useChatStore((state) => state.busy)
  const loadSessions = useChatStore((state) => state.loadSessions)
  const workspacesLoaded = useWorkspaceStore((state) => state.loaded)
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const loadWorkspaces = useWorkspaceStore((state) => state.loadWorkspaces)
  const workspaceCount = useWorkspaceStore((state) => state.workspaces.length)
  const previousWorkspace = useRef<string | null | undefined>(undefined)
  const [renameId, setRenameId] = useState<string | null>(null)
  const [renameError, setRenameError] = useState<string | null>(null)
  const [deleteId, setDeleteId] = useState<string | null>(null)
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const [plan, setPlan] = useState<{ view: PlanView; sessionId: string | null } | null>(null)
  const [trayOpen, setTrayOpen] = useState(false)
  const navigate = useNavigate()
  const reviewMatch = useMatch('/sessions/:sessionId/review')
  const reviewSessionId = reviewMatch?.params.sessionId ?? null
  const docsMatch = useMatch('/docs')
  const docsOpen = docsMatch !== null
  if (
    plan &&
    (draftSelected || reviewSessionId !== null || docsOpen || plan.sessionId !== activeSessionId)
  ) {
    setPlan(null)
  }
  // Leaving the docs view resets the tray, so it starts closed on the next visit.
  if (!docsOpen && trayOpen) {
    setTrayOpen(false)
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
  const compactAgent = useChatStore((state) => state.compactAgent)
  const compactingSessionId = useChatStore((state) => state.compactingSessionId)
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
  const messages =
    activeSessionId && !draftSelected ? (messagesBySession[activeSessionId] ?? []) : []
  const echo =
    activeSessionId && !draftSelected && pendingEcho?.sessionId === activeSessionId
      ? pendingEcho.text
      : null
  const echoFiles =
    activeSessionId && !draftSelected && pendingEcho?.sessionId === activeSessionId
      ? pendingEcho.files
      : []
  const agentRunning = phase !== 'idle'
  const composerLocked = loading || busy || agentRunning
  const composerDraftKey = draftSelected || !activeSessionId ? 'draft' : activeSessionId
  const stopping =
    !draftSelected && activeSessionId !== null && stoppingSessionId === activeSessionId
  const fresh = messages.length === 0 && echo === null

  const renameTarget = renameId
    ? (sessions.find((session) => session.id === renameId) ?? null)
    : null
  const deleteTarget = deleteId
    ? (sessions.find((session) => session.id === deleteId) ?? null)
    : null
  const deleteTargetRunning = deleteTarget ? runningIds.includes(deleteTarget.id) : false

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

  async function handleConfirmDelete(id: string) {
    await removeSession(id)
    const message = useChatStore.getState().error
    if (message) {
      setDeleteError(message)
      return
    }
    setDeleteId(null)
  }

  const sessionTitle =
    loading && !activeSession && !draftSelected ? 'Loading…' : sessionDisplayTitle(activeSession)
  const mode: AgentMode = draftSelected || !activeSession ? draftMode : sessionMode(activeSession)
  const modeDefault = modeDefaults[mode]
  const defaultModelId = modeDefault.model ?? fallbackModelId
  const defaultEffort = modeDefault.effort ?? fallbackEffort
  const modelId = draftSelected || !activeSession ? draftModel : sessionModel(activeSession, mode)
  const effort = draftSelected || !activeSession ? draftEffort : sessionEffort(activeSession, mode)

  const openSession = useCallback(
    (id: string) => {
      setPlan(null)
      if (reviewSessionId) {
        navigate('/')
      } else if (docsOpen) {
        setTrayOpen(true)
      }
      void selectSession(id)
    },
    [docsOpen, navigate, reviewSessionId, selectSession]
  )
  const openDraft = useCallback(() => {
    setPlan(null)
    if (reviewSessionId) {
      navigate('/')
    } else if (docsOpen) {
      setTrayOpen(true)
    }
    selectDraft()
  }, [docsOpen, navigate, reviewSessionId, selectDraft])
  const askRename = useCallback((id: string) => {
    setRenameError(null)
    setRenameId(id)
  }, [])
  const askDelete = useCallback((id: string) => {
    setDeleteError(null)
    setDeleteId(id)
  }, [])

  // Attaching a line opens the tray and hands the composer a ready attachment
  // for the current draft key — the selected session, or `draft` when none is
  // selected, so the first send creates the row.
  const attachLine = useCallback(
    (file: FileAttachment) => {
      setTrayOpen(true)
      requestComposerAttachment(composerDraftKey, file)
    },
    [composerDraftKey]
  )

  const chat: ChatPanelProps = {
    messages,
    sessionId: activeSessionId,
    echo,
    echoFiles,
    phase,
    mode,
    deciding: busy,
    buildDisabled: composerLocked,
    onDecide: (callId, decision) => {
      if (activeSessionId) {
        void decideCall(activeSessionId, callId, decision)
      }
    },
    onBuild: (path) => {
      void buildPlan(path)
    },
    onViewPlan: (next) => {
      setPlan({ view: next, sessionId: activeSessionId })
    },
    disabled: composerLocked,
    pending: busy,
    running: agentRunning,
    stopping,
    onStop: () => void stopAgent(),
    onSubmit: sendInstruction,
    models,
    modelId,
    effort,
    defaultModelId,
    defaultEffort,
    onModeChange: (next) => void setModeChoice(next),
    onModelChange: (model) => void setModelChoice(model),
    onEffortChange: (next) => void setEffortChoice(next),
    draftKey: composerDraftKey,
    pendingText: echo,
    workspaceId: activeWorkspaceId,
    onCompact: () => void compactAgent(),
    compacting: compactingSessionId === activeSessionId
  }

  return (
    <div className={styles.shell}>
      <Sidebar
        sessions={sessions}
        activeSessionId={draftSelected ? '' : (activeSession?.id ?? '')}
        draftSelected={draftSelected}
        docsSelected={docsOpen}
        disabled={loading}
        runningSessionIds={new Set(runningIds)}
        awaitingSessionIds={new Set(awaitingIds)}
        onSelectSession={openSession}
        onNewSession={openDraft}
        onRenameSession={askRename}
        onDeleteSession={askDelete}
      />
      <div className={styles.main}>
        <ChatHeader sessionTitle={docsOpen ? 'Documentation' : sessionTitle} />
        <ErrorNotices />
        {reviewSessionId ? (
          <ReviewScreen key={reviewSessionId} sessionId={reviewSessionId} />
        ) : docsOpen ? (
          <DocsScreen
            key={activeWorkspaceId ?? 'none'}
            workspaceId={activeWorkspaceId}
            onAttachLine={attachLine}
          />
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
        ) : transcriptLoading && messages.length === 0 && echo === null ? (
          <div className={styles.loadingTranscript} role="status">
            <span className={styles.spinner} aria-hidden />
            Loading conversation
          </div>
        ) : fresh ? (
          <div className={styles.welcome}>
            <EmptyGreeting />
            <Composer
              placement="welcome"
              disabled={composerLocked}
              pending={busy}
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
              draftKey={composerDraftKey}
              pendingText={echo}
              workspaceId={activeWorkspaceId}
            />
          </div>
        ) : (
          <ChatPanel {...chat} showReviewStrip />
        )}
        {docsOpen ? (
          <ChatTray
            {...chat}
            open={trayOpen}
            onOpen={() => setTrayOpen(true)}
            onClose={() => setTrayOpen(false)}
            title={sessionTitle}
          />
        ) : null}
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
      {deleteTarget ? (
        <DeleteSessionDialog
          key={deleteTarget.id}
          open
          title={sessionDisplayTitle(deleteTarget)}
          running={deleteTargetRunning}
          busy={busy}
          error={deleteError}
          onCancel={() => {
            setDeleteError(null)
            setDeleteId(null)
          }}
          onDelete={() => void handleConfirmDelete(deleteTarget.id)}
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
    return () => {
      cancelled = true
    }
  }, [])

  return { models, fallbackModelId, fallbackEffort, modeDefaults }
}
