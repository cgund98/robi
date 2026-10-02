import { useNavigate } from 'react-router-dom'

import { sessionDisplayTitle, type ChatSession } from '../../api/sessions'
import styles from './Sidebar.module.css'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'

type SidebarProps = {
  sessions: ChatSession[]
  activeSessionId: string
  draftSelected?: boolean
  disabled?: boolean
  /** Sessions whose agent is still running. */
  runningSessionIds?: ReadonlySet<string>
  onSelectSession: (id: string) => void
  onNewSession: () => void
  onRenameSession: (id: string) => void
  onDeleteSession: (id: string) => void
}

export function Sidebar({
  sessions,
  activeSessionId,
  draftSelected = false,
  disabled = false,
  runningSessionIds,
  onSelectSession,
  onNewSession,
  onRenameSession,
  onDeleteSession
}: SidebarProps) {
  const navigate = useNavigate()

  return (
    <aside className={styles.sidebar}>
      <WorkspaceSwitcher />

      <button
        type="button"
        className={`${styles.navLink} ${draftSelected ? styles.navLinkActive : ''}`}
        onClick={onNewSession}
        disabled={disabled}
        aria-current={draftSelected ? 'page' : undefined}
      >
        <span className={styles.navIcon} aria-hidden>
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.75"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
            <path d="M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z" />
          </svg>
        </span>
        New chat
      </button>

      <button
        type="button"
        className={styles.navLink}
        disabled={disabled}
        onClick={() => navigate('/workspaces')}
      >
        <span className={styles.navIcon} aria-hidden>
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.75"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <rect x="3" y="3" width="7" height="7" rx="1.5" />
            <rect x="14" y="3" width="7" height="7" rx="1.5" />
            <rect x="3" y="14" width="7" height="7" rx="1.5" />
            <rect x="14" y="14" width="7" height="7" rx="1.5" />
          </svg>
        </span>
        Workspaces
      </button>

      <div className={styles.section}>
        <div className={styles.sectionLabel}>Recents</div>
        <ul className={styles.sessionList}>
          {sessions.map((session) => {
            const active = session.id === activeSessionId
            const title = sessionDisplayTitle(session)
            const running = runningSessionIds?.has(session.id) ?? false
            return (
              <li key={session.id} className={styles.sessionRow}>
                <button
                  type="button"
                  className={`${styles.sessionButton} ${active ? styles.sessionButtonActive : ''}`}
                  onClick={() => onSelectSession(session.id)}
                  title={running ? `${title} (running)` : title}
                  aria-label={running ? `${title}, agent running` : undefined}
                  disabled={disabled}
                >
                  {running ? <span className={styles.spinner} aria-hidden /> : null}
                  <span className={styles.sessionTitle}>{title}</span>
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
        <button
          type="button"
          className={styles.navLink}
          disabled={disabled}
          onClick={() => navigate('/settings/providers')}
        >
          <span className={styles.navIcon} aria-hidden>
            <svg
              width="18"
              height="18"
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
