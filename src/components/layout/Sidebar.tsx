import { sessionDisplayTitle, type ChatSession } from '../../api/sessions'
import styles from './Sidebar.module.css'

type SidebarProps = {
  sessions: ChatSession[]
  activeSessionId: string
  disabled?: boolean
  onSelectSession: (id: string) => void
  onNewSession: () => void
  onRenameSession: (id: string) => void
  onDeleteSession: (id: string) => void
}

export function Sidebar({
  sessions,
  activeSessionId,
  disabled = false,
  onSelectSession,
  onNewSession,
  onRenameSession,
  onDeleteSession
}: SidebarProps) {
  return (
    <aside className={styles.sidebar}>
      <div className={styles.brand}>
        <span className={styles.brandMark} aria-hidden>
          R
        </span>
        Robi
      </div>

      <button
        type="button"
        className={styles.newSession}
        onClick={onNewSession}
        disabled={disabled}
      >
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
            const title = sessionDisplayTitle(session)
            return (
              <li key={session.id} className={styles.sessionRow}>
                <button
                  type="button"
                  className={`${styles.sessionButton} ${active ? styles.sessionButtonActive : ''}`}
                  onClick={() => onSelectSession(session.id)}
                  title={title}
                  disabled={disabled}
                >
                  {title}
                </button>
                <div className={styles.sessionActions}>
                  <button
                    type="button"
                    className={styles.sessionAction}
                    onClick={() => onRenameSession(session.id)}
                    disabled={disabled}
                    title="Rename"
                    aria-label={`Rename ${title}`}
                  >
                    ✎
                  </button>
                  <button
                    type="button"
                    className={styles.sessionAction}
                    onClick={() => onDeleteSession(session.id)}
                    disabled={disabled}
                    title="Delete"
                    aria-label={`Delete ${title}`}
                  >
                    ×
                  </button>
                </div>
              </li>
            )
          })}
        </ul>
      </div>

      <div className={styles.footer}>
        <button type="button" className={styles.settings} disabled={disabled}>
          <span className={styles.settingsIcon} aria-hidden>
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.75"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
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
