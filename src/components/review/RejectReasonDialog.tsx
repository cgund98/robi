import { useRef, useState, type FormEvent } from 'react'
import * as Dialog from '@radix-ui/react-dialog'

import styles from './RejectReasonDialog.module.css'

type RejectReasonDialogProps = {
  open: boolean
  /** The reviewed path the reason is about. */
  path: string
  /** 1-based inclusive range of a rejected hunk. Null for a whole-file reject. */
  range?: { start: number; end: number } | null
  busy: boolean
  error: string | null
  onCancel: () => void
  onSubmit: (reason: string) => void
}

export function RejectReasonDialog({
  open,
  path,
  range = null,
  busy,
  error,
  onCancel,
  onSubmit
}: RejectReasonDialogProps) {
  const fieldRef = useRef<HTMLTextAreaElement>(null)
  const [reason, setReason] = useState('')
  const [localError, setLocalError] = useState<string | null>(null)

  const where =
    range === null
      ? path
      : range.start === range.end
        ? `${path} line ${range.start}`
        : `${path} lines ${range.start}-${range.end}`

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    const trimmed = reason.trim()
    if (trimmed.length === 0) {
      setLocalError('Add a reason for the model')
      return
    }
    setLocalError(null)
    onSubmit(trimmed)
  }

  function keepOpen(event: Event) {
    if (busy) {
      event.preventDefault()
    }
  }

  const message = localError ?? error

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
          onOpenAutoFocus={(event) => {
            event.preventDefault()
            fieldRef.current?.focus()
          }}
          onEscapeKeyDown={keepOpen}
          onPointerDownOutside={keepOpen}
        >
          <form onSubmit={handleSubmit}>
            <Dialog.Title className={styles.heading}>Reject with reason</Dialog.Title>
            <p className={styles.subhead}>
              Tell the model what to change in <span className={styles.path}>{where}</span>.
            </p>
            <label className={styles.field}>
              <span className={styles.label}>Feedback</span>
              <textarea
                ref={fieldRef}
                className={styles.textarea}
                value={reason}
                onChange={(event) => setReason(event.target.value)}
                placeholder="What should the model change, and why?"
                rows={4}
                disabled={busy}
                maxLength={4000}
              />
            </label>
            {message ? (
              <p className={styles.error} role="alert">
                {message}
              </p>
            ) : null}
            <div className={styles.actions}>
              <button type="button" className={styles.cancel} onClick={onCancel} disabled={busy}>
                Cancel
              </button>
              <button type="submit" className={styles.submit} disabled={busy}>
                Reject and send
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
