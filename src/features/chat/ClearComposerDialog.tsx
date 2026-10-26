import { Dialog } from '@kobalte/core/dialog'

import styles from './ClearComposerDialog.module.css'

function attachmentLine(count: number): string {
  if (count === 0) {
    return 'The text will be removed.'
  }
  if (count === 1) {
    return 'The text and 1 attachment will be removed.'
  }
  return `The text and ${count} attachments will be removed.`
}

export function ClearComposerDialog(props: {
  open: boolean
  attachmentCount: number
  onCancel: () => void
  onClear: () => void
}) {
  return (
    <Dialog
      open={props.open}
      onOpenChange={(next) => {
        if (!next) {
          props.onCancel()
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay class={styles.overlay} />
        <Dialog.Content class={styles.panel}>
          <Dialog.Title class={styles.heading}>Clear composer</Dialog.Title>
          <p class={styles.body}>{attachmentLine(props.attachmentCount)}</p>
          <p class={styles.muted}>This cannot be undone.</p>
          <div class={styles.actions}>
            <button type="button" class={styles.cancel} onClick={() => props.onCancel()}>
              Cancel
            </button>
            <button type="button" class={styles.clear} onClick={() => props.onClear()}>
              Clear
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  )
}
