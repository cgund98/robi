import { useRef, useState, type FormEvent } from 'react'
import * as Dialog from '@radix-ui/react-dialog'

import styles from './RenameSessionDialog.module.css'

type RenameSessionDialogProps = {
  open: boolean
  initialTitle: string
  busy: boolean
  error: string | null
  onCancel: () => void
  onSave: (title: string) => void
}

export function RenameSessionDialog({
  open,
  initialTitle,
  busy,
  error,
  onCancel,
  onSave
}: RenameSessionDialogProps) {
  const inputRef = useRef<HTMLInputElement>(null)
  const [title, setTitle] = useState(initialTitle)
  const [localError, setLocalError] = useState<string | null>(null)

  function handleSubmit(event: FormEvent) {
    event.preventDefault()
    const trimmed = title.trim()
    if (trimmed.length === 0) {
      setLocalError('Title cannot be empty')
      return
    }
    setLocalError(null)
    onSave(trimmed)
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
            inputRef.current?.focus()
            inputRef.current?.select()
          }}
          onEscapeKeyDown={keepOpen}
          onPointerDownOutside={keepOpen}
        >
          <form onSubmit={handleSubmit}>
            <Dialog.Title className={styles.heading}>Rename chat</Dialog.Title>
            <label className={styles.field}>
              <span className={styles.label}>Title</span>
              <input
                ref={inputRef}
                className={styles.input}
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                disabled={busy}
                maxLength={200}
                autoComplete="off"
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
              <button type="submit" className={styles.save} disabled={busy}>
                Save
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
