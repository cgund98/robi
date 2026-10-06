/** @jsxImportSource solid-js */
import { For, Show, type Component } from 'solid-js'

import { CodeBracket, Eye } from '../../components/ui/icons'
import type { DocViewMode } from './docsCache'
import type { SaveStatus } from './useDocAutosave'
import styles from './DocToolbar.module.css'

const MODES: { id: DocViewMode; label: string; icon: Component<{ size?: number }> }[] = [
  { id: 'rendered', label: 'Preview', icon: Eye },
  { id: 'edit', label: 'Edit', icon: CodeBracket }
]

/** The word beside the document: what the last save did, or what it is doing. */
export function statusText(status: SaveStatus, dirty: boolean): string {
  switch (status.kind) {
    case 'saving':
      return 'Saving…'
    case 'saved':
      if (status.outcome === 'merged') {
        return 'Saved — merged with an agent edit'
      }
      if (status.outcome === 'created') {
        return 'Created'
      }
      return 'Saved'
    case 'out-of-sync':
      return 'Out of sync — re-sending'
    case 'error':
      return status.message
    default:
      return dirty ? 'Unsaved changes' : 'Saved'
  }
}

export function DocToolbar(props: {
  mode: DocViewMode
  onMode: (mode: DocViewMode) => void
  onSave: () => void
  dirty: boolean
  status: SaveStatus
  /** Set when an agent changed the open file while the buffer was dirty. */
  notice: string | null
}) {
  const tone = () => (props.status.kind === 'error' ? 'error' : 'plain')

  return (
    <div class={styles.bar}>
      <div class={styles.modes} role="radiogroup" aria-label="Document view">
        <For each={MODES}>
          {(item) => (
            <button
              type="button"
              role="radio"
              aria-checked={props.mode === item.id}
              class={props.mode === item.id ? `${styles.mode} ${styles.modeActive}` : styles.mode}
              onClick={() => props.onMode(item.id)}
            >
              <item.icon size={14} aria-hidden="true" />
              {item.label}
            </button>
          )}
        </For>
      </div>
      <Show when={props.notice}>
        <span class={styles.notice} role="status">
          {props.notice}
        </span>
      </Show>
      <div class={styles.saveWrap}>
        <span class={`${styles.status} ${styles[tone()]}`} role="status">
          <Show when={props.dirty}>
            <span class={styles.dot} aria-hidden="true" />
          </Show>
          {statusText(props.status, props.dirty)}
        </span>
        <Show when={props.mode === 'edit'}>
          <button type="button" class={styles.save} onClick={() => props.onSave()}>
            Save
          </button>
        </Show>
      </div>
    </div>
  )
}
