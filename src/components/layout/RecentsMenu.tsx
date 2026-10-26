import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { createMemo, createSignal, For, Show } from 'solid-js'

import { sessionDisplayTitle, type ChatSession } from '../../api/sessions'
import { ChevronDown } from '../ui/icons'
import styles from './RecentsMenu.module.css'
import { sessionMark, visibleRecents, type SessionMark } from './recents'

export type RecentsMenuProps = {
  /** The current session's title. It is also the trigger for this menu. */
  title: string
  sessions: ChatSession[]
  activeSessionId: string
  draftSelected: boolean
  /** In-memory turn phase per session, for the running mark. */
  phaseBySession: Record<string, string | undefined>
  disabled?: boolean
  onSelectSession: (id: string) => void
  onRenameSession: (id: string) => void
  onDeleteSession: (id: string) => void
}

/**
 * Recents, as a menu behind the session title.
 *
 * A dropdown is worse than a list once there is more than a screenful of
 * sessions, so this one ships with a filter field and a cap: the hidden count
 * tells the user to type rather than scroll.
 */
export function RecentsMenu(props: RecentsMenuProps) {
  const [query, setQuery] = createSignal('')
  let filterInput: HTMLInputElement | undefined

  const view = createMemo(() => visibleRecents(props.sessions, query()))

  return (
    <DropdownMenu onOpenChange={(open) => (!open ? setQuery('') : undefined)}>
      <DropdownMenu.Trigger class={styles.trigger} aria-label={`${props.title} — recents`}>
        <span class={styles.title}>{props.title}</span>
        <span class={styles.chevron} aria-hidden="true">
          <ChevronDown size={12} />
        </span>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          class={styles.panel}
          aria-label="Recents"
          onOpenAutoFocus={(event) => {
            event.preventDefault()
            filterInput?.focus()
          }}
        >
          <input
            ref={(element) => {
              filterInput = element
            }}
            class={styles.filter}
            type="search"
            value={query()}
            placeholder="Filter sessions"
            aria-label="Filter sessions"
            onInput={(event) => setQuery(event.currentTarget.value)}
            onKeyDown={(event) => event.stopPropagation()}
          />
          <Show
            when={view().rows.length > 0}
            fallback={<p class={styles.empty}>No sessions match.</p>}
          >
            <DropdownMenu.Group class={styles.list}>
              <For each={view().rows}>
                {(session) => (
                  <SessionRow
                    session={session}
                    active={!props.draftSelected && session.id === props.activeSessionId}
                    mark={sessionMark(session, props.phaseBySession[session.id])}
                    disabled={props.disabled ?? false}
                    onSelect={props.onSelectSession}
                    onRename={props.onRenameSession}
                    onDelete={props.onDeleteSession}
                  />
                )}
              </For>
            </DropdownMenu.Group>
          </Show>
          <Show when={view().hidden > 0}>
            <p class={styles.hidden}>
              {view().hidden} more {view().hidden === 1 ? 'session' : 'sessions'} — type to narrow
              the list.
            </p>
          </Show>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu>
  )
}

function SessionRow(props: {
  session: ChatSession
  active: boolean
  mark: SessionMark
  disabled: boolean
  onSelect: (id: string) => void
  onRename: (id: string) => void
  onDelete: (id: string) => void
}) {
  const title = () => sessionDisplayTitle(props.session)

  return (
    <DropdownMenu.Item
      class={props.active ? `${styles.row} ${styles.rowActive}` : styles.row}
      onSelect={() => props.onSelect(props.session.id)}
      title={
        props.mark === 'running'
          ? `${title()} (running)`
          : props.mark === 'awaiting'
            ? `${title()} (needs approval)`
            : title()
      }
      aria-label={
        props.mark === 'running'
          ? `${title()}, agent running`
          : props.mark === 'awaiting'
            ? `${title()}, waiting on approval`
            : undefined
      }
    >
      <Mark kind={props.mark} />
      <span class={styles.rowTitle}>{title()}</span>
      <span class={styles.rowActions}>
        <button
          type="button"
          class={styles.rowAction}
          title="Rename"
          aria-label={`Rename ${title()}`}
          disabled={props.disabled}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation()
            props.onRename(props.session.id)
          }}
        >
          ✎
        </button>
        <button
          type="button"
          class={styles.rowAction}
          title="Delete"
          aria-label={`Delete ${title()}`}
          disabled={props.disabled}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation()
            props.onDelete(props.session.id)
          }}
        >
          ×
        </button>
      </span>
    </DropdownMenu.Item>
  )
}

function Mark(props: { kind: SessionMark }) {
  return (
    <Show when={props.kind !== 'idle'}>
      <Show
        when={props.kind === 'running'}
        fallback={<span class={styles.approvalDot} aria-hidden="true" />}
      >
        <span class={styles.spinner} aria-hidden="true" />
      </Show>
    </Show>
  )
}
