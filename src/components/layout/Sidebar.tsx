import type { MockSession } from '../../mock/chat'
import styles from './Sidebar.module.css'

type SidebarProps = {
  sessions: MockSession[]
  activeSessionId: string
  onSelectSession: (id: string) => void
  onNewSession: () => void
}

export function Sidebar({
  sessions,
  activeSessionId,
  onSelectSession,
  onNewSession
}: SidebarProps) {
  return (
    <aside className={styles.sidebar}>
      <div className={styles.brand}>
        <span className={styles.brandMark} aria-hidden>
          R
        </span>
        Robi
      </div>

      <button type="button" className={styles.newSession} onClick={onNewSession}>
        <span className={styles.newSessionIcon} aria-hidden>
          +
        </span>
        New session
      </button>

      <div className={styles.section}>
        <div className={styles.sectionLabel}>Recents</div>
        <ul className={styles.sessionList}>
          {sessions.map((session) => {
            const active = session.id === activeSessionId
            return (
              <li key={session.id}>
                <button
                  type="button"
                  className={`${styles.sessionButton} ${active ? styles.sessionButtonActive : ''}`}
                  onClick={() => onSelectSession(session.id)}
                  title={session.title}
                >
                  {session.title}
                </button>
              </li>
            )
          })}
        </ul>
      </div>

      <div className={styles.footer}>
        <button type="button" className={styles.settings}>
          <span className={styles.settingsIcon} aria-hidden>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z" />
              <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3h.1a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8v.1a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z" />
            </svg>
          </span>
          Settings
        </button>
      </div>
    </aside>
  )
}
