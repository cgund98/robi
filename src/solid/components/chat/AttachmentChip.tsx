/** @jsxImportSource solid-js */
import { DocumentText } from '../../ui/icons'
import { Show } from 'solid-js'

import styles from '../../../components/chat/AttachmentChip.module.css'

export function AttachmentChip(props: {
  name: string
  path?: string | null
  startLine?: number | null
  endLine?: number | null
  onRemove?: () => void
}) {
  const range = () =>
    props.startLine != null && props.endLine != null
      ? props.startLine === props.endLine
        ? ` (${props.startLine})`
        : ` (${props.startLine}-${props.endLine})`
      : ''
  const origin = () => props.path ?? 'outside the workspace'

  return (
    <div class={styles.chip} title={`${props.name}${range()} — ${origin()}`}>
      <DocumentText size={14} class={styles.icon} aria-hidden="true" />
      <span class={styles.name}>{props.name}</span>
      <Show when={range()}>
        <span class={styles.range}>{range()}</span>
      </Show>
      <Show when={props.onRemove}>
        <button
          type="button"
          class={styles.remove}
          aria-label={`Remove ${props.name}`}
          onClick={() => props.onRemove?.()}
        >
          ×
        </button>
      </Show>
    </div>
  )
}
