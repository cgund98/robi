import * as Dialog from '@radix-ui/react-dialog'

import styles from './DeleteSessionDialog.module.css'

type DeleteSessionDialogProps = {
  open: boolean
  /** Display title of the session. The caller resolves the "New chat" fallback. */
  title: string
  /** True while the session's agent is running, so the request may wait on a stop. */
  running: boolean
  /** True while the DELETE request is in flight. */
  busy: boolean
  error: string | null
  onCancel: () => void
  onDelete: () => void
}

export function DeleteSessionDialog({
  open,
  title,
  running,
  busy,
  error,
  onCancel,
  onDelete
}: DeleteSessionDialogProps) {
  function keepOpen(event: Event) {
    if (busy) {
      event.preventDefault()
    }
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) {
          onCancel()
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content
          className={styles.panel}
          onEscapeKeyDown={keepOpen}
          onPointerDownOutside={keepOpen}
        >
          <Dialog.Title className={styles.heading}>Delete chat</Dialog.Title>
          <p className={styles.body}>
            Delete <span className={styles.name}>“{title}”</span>? This cannot be undone.
          </p>
          {running ? <p className={styles.running}>A running turn will be stopped.</p> : null}
          {error ? (
            <p className={styles.error} role="alert">
              {error}
            </p>
          ) : null}
          <div className={styles.actions}>
            <button type="button" className={styles.cancel} onClick={onCancel} disabled={busy}>
              Cancel
            </button>
            <button type="button" className={styles.delete} onClick={onDelete} disabled={busy}>
              {busy ? <span className={styles.spinner} aria-hidden /> : null}
              Delete
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
