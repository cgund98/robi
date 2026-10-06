/** @jsxImportSource solid-js */
import { Dialog } from '@kobalte/core/dialog'
import { Show } from 'solid-js'

import styles from '../../../components/chat/DeleteSessionDialog.module.css'

export function DeleteSessionDialog(props: {
  open: boolean
  title: string
  running: boolean
  busy: boolean
  error: string | null
  onCancel: () => void
  onDelete: () => void
}) {
  function keepOpen(event: Event) {
    if (props.busy) {
      event.preventDefault()
    }
  }

  return (
    <Dialog
      open={props.open}
      onOpenChange={(next) => {
        if (!next && !props.busy) {
          props.onCancel()
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay class={styles.overlay} />
        <Dialog.Content
          class={styles.panel}
          onEscapeKeyDown={keepOpen}
          onPointerDownOutside={keepOpen}
        >
          <Dialog.Title class={styles.heading}>Delete chat</Dialog.Title>
          <p class={styles.body}>
            Delete <span class={styles.name}>“{props.title}”</span>? This cannot be undone.
          </p>
          <Show when={props.running}>
            <p class={styles.running}>A running turn will be stopped.</p>
          </Show>
          <Show when={props.error}>
            <p class={styles.error} role="alert">
              {props.error}
            </p>
          </Show>
          <div class={styles.actions}>
            <button
              type="button"
              class={styles.cancel}
              onClick={() => props.onCancel()}
              disabled={props.busy}
            >
              Cancel
            </button>
            <button
              type="button"
              class={styles.delete}
              onClick={() => props.onDelete()}
              disabled={props.busy}
            >
              <Show when={props.busy}>
                <span class={styles.spinner} aria-hidden="true" />
              </Show>
              Delete
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  )
}
