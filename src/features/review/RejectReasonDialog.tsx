/** @jsxImportSource solid-js */
import { Dialog } from '@kobalte/core/dialog'
import { createSignal, Show } from 'solid-js'

import styles from './RejectReasonDialog.module.css'

export function RejectReasonDialog(props: {
  open: boolean
  path: string
  range?: { start: number; end: number } | null
  busy: boolean
  error: string | null
  onCancel: () => void
  onSubmit: (reason: string) => void
}) {
  const [reason, setReason] = createSignal('')
  const [localError, setLocalError] = createSignal<string | null>(null)
  const message = () => localError() ?? props.error
  const where = () => {
    const range = props.range ?? null
    if (range === null) {
      return props.path
    }
    return range.start === range.end
      ? `${props.path} line ${range.start}`
      : `${props.path} lines ${range.start}-${range.end}`
  }

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
              const trimmed = reason().trim()
              if (trimmed.length === 0) {
                setLocalError('Add a reason for the model')
                return
              }
              setLocalError(null)
              props.onSubmit(trimmed)
            }}
          >
            <Dialog.Title class={styles.heading}>Reject with reason</Dialog.Title>
            <p class={styles.subhead}>
              Tell the model what to change in <span class={styles.path}>{where()}</span>.
            </p>
            <label class={styles.field}>
              <span class={styles.label}>Feedback</span>
              <textarea
                class={styles.textarea}
                value={reason()}
                onInput={(event) => setReason(event.currentTarget.value)}
                placeholder="What should the model change, and why?"
                rows={4}
                disabled={props.busy}
                maxLength={4000}
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
              <button type="submit" class={styles.submit} disabled={props.busy}>
                Reject and send
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  )
}
