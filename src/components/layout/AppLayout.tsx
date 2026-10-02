import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { sessionDisplayTitle } from '../../api/sessions'
import { useAgentEventsSSE } from '../../app/useAgentEventsSSE'
import { useChatStore, type AgentPhase } from '../../state/chatStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import { ChatHeader } from '../chat/ChatHeader'
import { Composer } from '../chat/Composer'
import { RenameSessionDialog } from '../chat/RenameSessionDialog'
import { Transcript } from '../chat/Transcript'
import { Sidebar } from './Sidebar'
import styles from './AppLayout.module.css'

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
  const sendInstruction = useChatStore((state) => state.sendInstruction)
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
        disabled={loading || busy}
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
        <ChatHeader sessionTitle={sessionTitle} />
        <Transcript messages={messages} echo={echo} phase={phase} />
        <Composer disabled={composerLocked} phase={phase} onSubmit={sendInstruction} />
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
