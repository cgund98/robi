/** @jsxImportSource solid-js */
import { useNavigate } from '@solidjs/router'
import { Cog, PencilSquare, Squares } from '../ui/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'

import { sessionDisplayTitle, type ChatSession } from '../../api/sessions'
import styles from './Sidebar.module.css'
import { HistoryNav } from './HistoryNav'
import { ViewToggle } from './ViewToggle'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'

type SidebarProps = {
  sessions: ChatSession[]
  activeSessionId: string
  draftSelected?: boolean
  docsSelected?: boolean
  disabled?: boolean
  runningSessionIds?: ReadonlySet<string>
  awaitingSessionIds?: ReadonlySet<string>
  onSelectSession: (id: string) => void
  onNewSession: () => void
  onRenameSession: (id: string) => void
  onDeleteSession: (id: string) => void
}

const INITIAL_VISIBLE = 5
const SHOW_MORE_STEP = 10

export function Sidebar(props: SidebarProps) {
  const navigate = useNavigate()
  const [visibleCount, setVisibleCount] = createSignal(INITIAL_VISIBLE)
  let previousNewestId = props.sessions[0]?.id

  createEffect(() => {
    const newestId = props.sessions[0]?.id
    if (previousNewestId === newestId) {
      return
    }
    const previous = previousNewestId
    previousNewestId = newestId
    if (previous !== undefined && !props.sessions.some((session) => session.id === previous)) {
      setVisibleCount(INITIAL_VISIBLE)
    }
  })

  const shownCount = createMemo(() => {
    const activeIndex = props.sessions.findIndex((session) => session.id === props.activeSessionId)
    return Math.max(visibleCount(), activeIndex + 1)
  })
  const visibleSessions = createMemo(() => props.sessions.slice(0, shownCount()))
  const hiddenCount = () => props.sessions.length - visibleSessions().length
  const visibleIds = createMemo(() => visibleSessions().map((session) => session.id))

  return (
    <aside class={styles.sidebar}>
      <div class={styles.navTools}>
        <HistoryNav />
        <ViewToggle docsOpen={props.docsSelected ?? false} />
      </div>

      <WorkspaceSwitcher />

      <button
        type="button"
        class={`${styles.navLink} ${props.draftSelected ? styles.navLinkActive : ''}`}
        onClick={() => props.onNewSession()}
        disabled={props.disabled}
        aria-current={props.draftSelected ? 'page' : undefined}
      >
        <span class={styles.navIcon} aria-hidden="true">
          <PencilSquare size={18} />
        </span>
        New chat
      </button>

      <button
        type="button"
        class={styles.navLink}
        disabled={props.disabled}
        onClick={() => navigate('/workspaces')}
      >
        <span class={styles.navIcon} aria-hidden="true">
          <Squares size={18} />
        </span>
        Workspaces
      </button>

      <div class={styles.section}>
        <div class={styles.sectionLabel}>Recents</div>
        <ul class={styles.sessionList}>
          <For each={visibleIds()}>
            {(id) => (
              <SessionRow
                id={id}
                sessions={props.sessions}
                active={id === props.activeSessionId}
                running={props.runningSessionIds?.has(id) ?? false}
                awaiting={
                  !(props.runningSessionIds?.has(id) ?? false) &&
                  (props.awaitingSessionIds?.has(id) ?? false)
                }
                disabled={props.disabled ?? false}
                onSelect={props.onSelectSession}
                onRename={props.onRenameSession}
                onDelete={props.onDeleteSession}
              />
            )}
          </For>
        </ul>
        <Show when={hiddenCount() > 0}>
          <button
            type="button"
            class={styles.showMore}
            onClick={() => setVisibleCount(shownCount() + SHOW_MORE_STEP)}
            disabled={props.disabled}
          >
            Show more
          </button>
        </Show>
      </div>

      <div class={styles.footer}>
        <button
          type="button"
          class={styles.navLink}
          disabled={props.disabled}
          onClick={() => navigate('/settings/providers')}
        >
          <span class={styles.navIcon} aria-hidden="true">
            <Cog size={18} />
          </span>
          Settings
        </button>
      </div>
    </aside>
  )
}

function SessionRow(props: {
  id: string
  sessions: ChatSession[]
  active: boolean
  running: boolean
  awaiting: boolean
  disabled: boolean
  onSelect: (id: string) => void
  onRename: (id: string) => void
  onDelete: (id: string) => void
}) {
  const title = () => sessionDisplayTitle(props.sessions.find((session) => session.id === props.id))

  return (
    <li class={styles.sessionRow}>
      <button
        type="button"
        class={`${styles.sessionButton} ${props.active ? styles.sessionButtonActive : ''}`}
        onClick={() => props.onSelect(props.id)}
        title={
          props.running
            ? `${title()} (running)`
            : props.awaiting
              ? `${title()} (needs approval)`
              : title()
        }
        aria-label={
          props.running
            ? `${title()}, agent running`
            : props.awaiting
              ? `${title()}, waiting on approval`
              : undefined
        }
        disabled={props.disabled}
      >
        <Show
          when={props.running}
          fallback={
            <Show when={props.awaiting}>
              <ApprovalDot />
            </Show>
          }
        >
          <RunningSpinner />
        </Show>
        <span class={styles.sessionTitle}>{title()}</span>
      </button>
      <div class={styles.sessionActions}>
        <button
          type="button"
          class={styles.sessionAction}
          onClick={() => props.onRename(props.id)}
          disabled={props.disabled}
          title="Rename"
          aria-label={`Rename ${title()}`}
        >
          ✎
        </button>
        <button
          type="button"
          class={styles.sessionAction}
          onClick={() => props.onDelete(props.id)}
          disabled={props.disabled}
          title="Delete"
          aria-label={`Delete ${title()}`}
        >
          ×
        </button>
      </div>
    </li>
  )
}

function RunningSpinner() {
  return <span class={styles.spinner} aria-hidden="true" />
}

function ApprovalDot() {
  return <span class={styles.approvalDot} aria-hidden="true" />
}
