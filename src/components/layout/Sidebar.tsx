import { memo, useEffect, useRef, useState } from 'react'
import { BookOpen, LayoutGrid, Settings, SquarePen } from 'lucide-react'
import { useNavigate } from 'react-router-dom'

import { sessionDisplayTitle, type ChatSession } from '../../api/sessions'
import styles from './Sidebar.module.css'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'

type SidebarProps = {
  sessions: ChatSession[]
  activeSessionId: string
  draftSelected?: boolean
  docsSelected?: boolean
  disabled?: boolean
  /** Sessions whose agent is still running. */
  runningSessionIds?: ReadonlySet<string>
  /** Sessions waiting on a tool approval. */
  awaitingSessionIds?: ReadonlySet<string>
  onSelectSession: (id: string) => void
  onNewSession: () => void
  onRenameSession: (id: string) => void
  onDeleteSession: (id: string) => void
}

const SessionRow = memo(function SessionRow({
  id,
  title,
  active,
  running,
  awaiting,
  disabled,
  onSelect,
  onRename,
  onDelete
}: {
  id: string
  title: string
  active: boolean
  running: boolean
  awaiting: boolean
  disabled: boolean
  onSelect: (id: string) => void
  onRename: (id: string) => void
  onDelete: (id: string) => void
}) {
  return (
    <li className={styles.sessionRow}>
      <button
        type="button"
        className={`${styles.sessionButton} ${active ? styles.sessionButtonActive : ''}`}
        onClick={() => onSelect(id)}
        title={running ? `${title} (running)` : awaiting ? `${title} (needs approval)` : title}
        aria-label={
          running
            ? `${title}, agent running`
            : awaiting
              ? `${title}, waiting on approval`
              : undefined
        }
        disabled={disabled}
      >
        {running ? <RunningSpinner /> : awaiting ? <ApprovalDot /> : null}
        <span className={styles.sessionTitle}>{title}</span>
      </button>
      <div className={styles.sessionActions}>
        <button
          type="button"
          className={styles.sessionAction}
          onClick={() => onRename(id)}
          disabled={disabled}
          title="Rename"
          aria-label={`Rename ${title}`}
        >
          ✎
        </button>
        <button
          type="button"
          className={styles.sessionAction}
          onClick={() => onDelete(id)}
          disabled={disabled}
          title="Delete"
          aria-label={`Delete ${title}`}
        >
          ×
        </button>
      </div>
    </li>
  )
})

/** The ring is its own render. A row update must not reconcile it, or the spin restarts. */
const RunningSpinner = memo(function RunningSpinner() {
  return <span className={styles.spinner} aria-hidden />
})

const ApprovalDot = memo(function ApprovalDot() {
  return <span className={styles.approvalDot} aria-hidden />
})

const INITIAL_VISIBLE = 5
const SHOW_MORE_STEP = 10

export function Sidebar({
  sessions,
  activeSessionId,
  draftSelected = false,
  docsSelected = false,
  disabled = false,
  runningSessionIds,
  awaitingSessionIds,
  onSelectSession,
  onNewSession,
  onRenameSession,
  onDeleteSession
}: SidebarProps) {
  const navigate = useNavigate()
  const [visibleCount, setVisibleCount] = useState(INITIAL_VISIBLE)
  const newestId = sessions[0]?.id
  const previousNewestId = useRef(newestId)

  useEffect(() => {
    if (previousNewestId.current === newestId) {
      return
    }
    const previous = previousNewestId.current
    previousNewestId.current = newestId
    if (previous !== undefined && !sessions.some((session) => session.id === previous)) {
      setVisibleCount(INITIAL_VISIBLE)
    }
  }, [newestId, sessions])

  const activeIndex = sessions.findIndex((session) => session.id === activeSessionId)
  const shownCount = Math.max(visibleCount, activeIndex + 1)
  const visibleSessions = sessions.slice(0, shownCount)
  const hiddenCount = sessions.length - visibleSessions.length

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
          <SquarePen size={18} strokeWidth={1.75} />
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
          <LayoutGrid size={18} strokeWidth={1.75} />
        </span>
        Workspaces
      </button>

      <button
        type="button"
        className={`${styles.navLink} ${docsSelected ? styles.navLinkActive : ''}`}
        disabled={disabled}
        onClick={() => navigate('/docs')}
        aria-current={docsSelected ? 'page' : undefined}
      >
        <span className={styles.navIcon} aria-hidden>
          <BookOpen size={18} strokeWidth={1.75} />
        </span>
        Documentation
      </button>

      <div className={styles.section}>
        <div className={styles.sectionLabel}>Recents</div>
        <ul className={styles.sessionList}>
          {visibleSessions.map((session) => (
            <SessionRow
              key={session.id}
              id={session.id}
              title={sessionDisplayTitle(session)}
              active={session.id === activeSessionId}
              running={runningSessionIds?.has(session.id) ?? false}
              awaiting={
                !(runningSessionIds?.has(session.id) ?? false) &&
                (awaitingSessionIds?.has(session.id) ?? false)
              }
              disabled={disabled}
              onSelect={onSelectSession}
              onRename={onRenameSession}
              onDelete={onDeleteSession}
            />
          ))}
        </ul>
        {hiddenCount > 0 ? (
          <button
            type="button"
            className={styles.showMore}
            onClick={() => setVisibleCount(shownCount + SHOW_MORE_STEP)}
            disabled={disabled}
          >
            Show more
          </button>
        ) : null}
      </div>

      <div className={styles.footer}>
        <button
          type="button"
          className={styles.navLink}
          disabled={disabled}
          onClick={() => navigate('/settings/providers')}
        >
          <span className={styles.navIcon} aria-hidden>
            <Settings size={18} strokeWidth={1.75} />
          </span>
          Settings
        </button>
      </div>
    </aside>
  )
}
