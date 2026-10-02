import { useCallback, useEffect, useMemo, useState } from 'react'

import {
  createSession,
  deleteSession,
  listSessions,
  sessionDisplayTitle,
  updateSession,
  type ChatSession
} from '../../api/sessions'
import { ChatHeader } from '../chat/ChatHeader'
import { Composer } from '../chat/Composer'
import { Transcript } from '../chat/Transcript'
import { Sidebar } from './Sidebar'
import { MOCK_TRANSCRIPT } from '../../mock/chat'
import { SHELL_WORKSPACE } from '../../workspace'
import styles from './AppLayout.module.css'

export function AppLayout() {
  const [sessions, setSessions] = useState<ChatSession[]>([])
  const [activeSessionId, setActiveSessionId] = useState('')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const activeSession = useMemo(
    () => sessions.find((session) => session.id === activeSessionId) ?? sessions[0],
    [sessions, activeSessionId]
  )

  useEffect(() => {
    let cancelled = false

    void listSessions(SHELL_WORKSPACE.id)
      .then((next) => {
        if (cancelled) {
          return
        }
        setSessions(next)
        setActiveSessionId(next[0]?.id ?? '')
        setError(null)
        setLoading(false)
      })
      .catch((err: unknown) => {
        if (cancelled) {
          return
        }
        setError(err instanceof Error ? err.message : 'Failed to load chat sessions')
        setSessions([])
        setActiveSessionId('')
        setLoading(false)
      })

    return () => {
      cancelled = true
    }
  }, [])

  const reloadSessions = useCallback(async () => {
    setBusy(true)
    try {
      const next = await listSessions(SHELL_WORKSPACE.id)
      setSessions(next)
      setActiveSessionId((current) => {
        if (current && next.some((session) => session.id === current)) {
          return current
        }
        return next[0]?.id ?? ''
      })
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load chat sessions')
    } finally {
      setBusy(false)
    }
  }, [])

  async function handleNewSession() {
    setBusy(true)
    setError(null)
    try {
      const created = await createSession(SHELL_WORKSPACE.id)
      setSessions((current) => [created, ...current.filter((session) => session.id !== created.id)])
      setActiveSessionId(created.id)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create chat session')
    } finally {
      setBusy(false)
    }
  }

  async function handleRenameSession(id: string) {
    const current = sessions.find((session) => session.id === id)
    const nextTitle = window.prompt('Rename session', sessionDisplayTitle(current))
    if (nextTitle == null) {
      return
    }
    const trimmed = nextTitle.trim()
    if (trimmed.length === 0) {
      setError('Title cannot be empty')
      return
    }

    setBusy(true)
    setError(null)
    try {
      const updated = await updateSession(id, trimmed)
      setSessions((current) => current.map((session) => (session.id === id ? updated : session)))
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to rename chat session')
    } finally {
      setBusy(false)
    }
  }

  async function handleDeleteSession(id: string) {
    const current = sessions.find((session) => session.id === id)
    const label = sessionDisplayTitle(current)
    if (!window.confirm(`Delete “${label}”? This cannot be undone.`)) {
      return
    }

    setBusy(true)
    setError(null)
    try {
      await deleteSession(id)
      setSessions((current) => {
        const next = current.filter((session) => session.id !== id)
        setActiveSessionId((active) => {
          if (active !== id) {
            return active
          }
          return next[0]?.id ?? ''
        })
        return next
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete chat session')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className={styles.shell}>
      <Sidebar
        sessions={sessions}
        activeSessionId={activeSession?.id ?? ''}
        disabled={loading || busy}
        onSelectSession={setActiveSessionId}
        onNewSession={() => void handleNewSession()}
        onRenameSession={(id) => void handleRenameSession(id)}
        onDeleteSession={(id) => void handleDeleteSession(id)}
      />
      <div className={styles.main}>
        {error ? (
          <div className={styles.banner} role="alert">
            <span>{error}</span>
            <button
              type="button"
              className={styles.bannerRetry}
              onClick={() => void reloadSessions()}
            >
              Retry
            </button>
          </div>
        ) : null}
        <ChatHeader
          workspace={SHELL_WORKSPACE.label}
          sessionTitle={loading && !activeSession ? 'Loading…' : sessionDisplayTitle(activeSession)}
        />
        <Transcript items={MOCK_TRANSCRIPT} />
        <Composer />
      </div>
    </div>
  )
}
