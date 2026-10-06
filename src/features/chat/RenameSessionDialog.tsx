/** @jsxImportSource solid-js */
import { Dialog } from '@kobalte/core/dialog'
import { createSignal, Show } from 'solid-js'

import styles from './RenameSessionDialog.module.css'

export function RenameSessionDialog(props: {
  open: boolean
  initialTitle: string
  busy: boolean
  error: string | null
  onCancel: () => void
  onSave: (title: string) => void
}) {
  const [title, setTitle] = createSignal(props.initialTitle)
  const [localError, setLocalError] = createSignal<string | null>(null)
  const message = () => localError() ?? props.error

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
          <form
            onSubmit={(event) => {
              event.preventDefault()
              const trimmed = title().trim()
              if (trimmed.length === 0) {
                setLocalError('Title cannot be empty')
                return
              }
              setLocalError(null)
              props.onSave(trimmed)
            }}
          >
            <Dialog.Title class={styles.heading}>Rename chat</Dialog.Title>
            <label class={styles.field}>
              <span class={styles.label}>Title</span>
              <input
                class={styles.input}
                value={title()}
                onInput={(event) => setTitle(event.currentTarget.value)}
                disabled={props.busy}
                maxLength={200}
                autocomplete="off"
              />
            </label>
            <Show when={message()}>
              <p class={styles.error} role="alert">
                {message()}
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
              <button type="submit" class={styles.save} disabled={props.busy}>
                Save
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  )
}
