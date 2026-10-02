import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { listModels, type CatalogModel } from '../../api/models'
import { sessionDisplayTitle, type ChatSession } from '../../api/sessions'
import { getSetting, SETTING_KEYS } from '../../api/settings'
import { useAgentEventsSSE } from '../../app/useAgentEventsSSE'
import { useChatStore, type AgentPhase } from '../../state/chatStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import { ChatHeader } from '../chat/ChatHeader'
import { Composer } from '../chat/Composer'
import { EmptyGreeting } from '../chat/EmptyGreeting'
import { RenameSessionDialog } from '../chat/RenameSessionDialog'
import { Transcript } from '../chat/Transcript'
import { Sidebar } from './Sidebar'
import styles from './AppLayout.module.css'

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
  const navigate = useNavigate()
  const selectSession = useChatStore((state) => state.selectSession)
  const selectDraft = useChatStore((state) => state.selectDraft)
  const draftModel = useChatStore((state) => state.draftModel)
  const draftEffort = useChatStore((state) => state.draftEffort)
  const setModelChoice = useChatStore((state) => state.setModelChoice)
  const setEffortChoice = useChatStore((state) => state.setEffortChoice)
  const sendInstruction = useChatStore((state) => state.sendInstruction)
  const { models, defaultModelId, defaultEffort } = useModelDefaults()
  const decideCall = useChatStore((state) => state.decideCall)
  const renameSession = useChatStore((state) => state.renameSession)
  const removeSession = useChatStore((state) => state.removeSession)

  useAgentEventsSSE()

  useEffect(() => {
    void loadWorkspaces()
  }, [loadWorkspaces])

  useEffect(() => {
    if (workspacesLoaded && workspaceCount === 0) {
      navigate('/workspaces', { replace: true })
    }
  }, [workspacesLoaded, workspaceCount, navigate])

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
  const composerLocked = loading || busy || phase !== 'idle'
  const fresh = messages.length === 0 && echo === null

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

  return (
    <div className={styles.shell}>
      <Sidebar
        sessions={sessions}
        activeSessionId={draftSelected ? '' : (activeSession?.id ?? '')}
        draftSelected={draftSelected}
        disabled={loading || busy}
        runningSessionIds={runningSessionIds(sessions, phaseBySession)}
        onSelectSession={(id) => void selectSession(id)}
        onNewSession={selectDraft}
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
        {fresh ? (
          <div className={styles.welcome}>
            <EmptyGreeting />
            <Composer
              placement="welcome"
              disabled={composerLocked}
              phase={phase}
              onSubmit={sendInstruction}
              models={models}
              modelId={draftSelected || !activeSession ? draftModel : sessionModel(activeSession)}
              effort={draftSelected || !activeSession ? draftEffort : sessionEffort(activeSession)}
              defaultModelId={defaultModelId}
              defaultEffort={defaultEffort}
              onModelChange={(model) => void setModelChoice(model)}
              onEffortChange={(effort) => void setEffortChoice(effort)}
            />
          </div>
        ) : (
          <>
            <ChatHeader sessionTitle={sessionTitle} />
            <Transcript
              messages={messages}
              echo={echo}
              phase={phase}
              deciding={busy}
              onDecide={(callId, decision) => {
                if (activeSessionId) {
                  void decideCall(activeSessionId, callId, decision)
                }
              }}
            />
            <Composer
              disabled={composerLocked}
              phase={phase}
              onSubmit={sendInstruction}
              models={models}
              modelId={draftSelected || !activeSession ? draftModel : sessionModel(activeSession)}
              effort={draftSelected || !activeSession ? draftEffort : sessionEffort(activeSession)}
              defaultModelId={defaultModelId}
              defaultEffort={defaultEffort}
              onModelChange={(model) => void setModelChoice(model)}
              onEffortChange={(effort) => void setEffortChoice(effort)}
            />
          </>
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

function sessionModel(session: ChatSession): string | null {
  return session.model_config.model ?? null
}

function sessionEffort(session: ChatSession): string | null {
  return session.model_config.reasoning_effort ?? null
}

function useModelDefaults(): {
  models: CatalogModel[]
  defaultModelId: string
  defaultEffort: string | null
} {
  const [models, setModels] = useState<CatalogModel[]>([])
  const [defaultModelId, setDefaultModelId] = useState('glm-5.3')
  const [defaultEffort, setDefaultEffort] = useState<string | null>(null)

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
          setDefaultModelId(setting.value)
        }
      })
      .catch(() => {})
    void getSetting(SETTING_KEYS.reasoningEffort)
      .then((setting) => {
        if (!cancelled) {
          setDefaultEffort(setting.value)
        }
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [])

  return { models, defaultModelId, defaultEffort }
}
