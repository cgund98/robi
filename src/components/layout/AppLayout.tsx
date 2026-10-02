import { useMemo, useState } from 'react'

import { ChatHeader } from '../chat/ChatHeader'
import { Composer } from '../chat/Composer'
import { Transcript } from '../chat/Transcript'
import { Sidebar } from './Sidebar'
import { MOCK_SESSIONS, MOCK_TRANSCRIPT, MOCK_WORKSPACE, type MockSession } from '../../mock/chat'
import styles from './AppLayout.module.css'

export function AppLayout() {
  const [sessions, setSessions] = useState<MockSession[]>(MOCK_SESSIONS)
  const [activeSessionId, setActiveSessionId] = useState(MOCK_SESSIONS[0]?.id ?? '')

  const activeSession = useMemo(
    () => sessions.find((session) => session.id === activeSessionId) ?? sessions[0],
    [sessions, activeSessionId]
  )

  function handleNewSession() {
    const id = `s${Date.now()}`
    const next: MockSession = { id, title: 'New session' }
    setSessions((current) => [next, ...current])
    setActiveSessionId(id)
  }

  return (
    <div className={styles.shell}>
      <Sidebar
        sessions={sessions}
        activeSessionId={activeSession?.id ?? ''}
        onSelectSession={setActiveSessionId}
        onNewSession={handleNewSession}
      />
      <div className={styles.main}>
        <ChatHeader
          workspace={MOCK_WORKSPACE}
          sessionTitle={activeSession?.title ?? 'New session'}
        />
        <Transcript items={MOCK_TRANSCRIPT} />
        <Composer />
      </div>
    </div>
  )
}
