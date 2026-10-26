import { useNavigate } from '@solidjs/router'

import type { ChatSession } from '../../api/sessions'
import type { AgentPhase } from '../../state/chatStore'
import { Cog, PencilSquare } from '../ui/icons'
import { HistoryNav } from './HistoryNav'
import { OverflowMenu } from './OverflowMenu'
import { RecentsMenu } from './RecentsMenu'
import styles from './TopBar.module.css'
import { WorkspaceSwitcher } from './WorkspaceSwitcher'

export type TopBarProps = {
  /** The open session's title. It is also the Recents trigger. */
  sessionTitle: string
  sessions: ChatSession[]
  activeSessionId: string
  draftSelected: boolean
  phaseBySession: Record<string, AgentPhase>
  disabled?: boolean
  onSelectSession: (id: string) => void
  onNewSession: () => void
  onRenameSession: (id: string) => void
  onDeleteSession: (id: string) => void
}

/**
 * The one bar on the chat shell: navigation where the window chrome already is.
 *
 * It draws over the window frame's strip, so the transcript starts on the next
 * row. The primary actions stay visible; the title truncates before any control
 * does, and the secondary chrome folds into the overflow menu.
 */
export function TopBar(props: TopBarProps) {
  const navigate = useNavigate()

  return (
    <header class={styles.bar} data-tauri-drag-region="deep">
      <div class={styles.leading}>
        <HistoryNav />
        <WorkspaceSwitcher compact />
      </div>
      <div class={styles.center}>
        <RecentsMenu
          title={props.sessionTitle}
          sessions={props.sessions}
          activeSessionId={props.activeSessionId}
          draftSelected={props.draftSelected}
          phaseBySession={props.phaseBySession}
          disabled={props.disabled}
          onSelectSession={props.onSelectSession}
          onRenameSession={props.onRenameSession}
          onDeleteSession={props.onDeleteSession}
        />
      </div>
      <div class={styles.actions}>
        <button
          type="button"
          class={styles.iconButton}
          aria-label="New chat"
          title="New chat"
          disabled={props.disabled}
          onClick={() => props.onNewSession()}
        >
          <PencilSquare size={16} />
        </button>
        <button
          type="button"
          class={styles.iconButton}
          aria-label="Settings"
          title="Settings"
          onClick={() => navigate('/settings/general')}
        >
          <Cog size={16} />
        </button>
        <OverflowMenu />
      </div>
    </header>
  )
}
