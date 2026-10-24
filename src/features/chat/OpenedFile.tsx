import { Dialog } from '@kobalte/core/dialog'
import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

import { readWorkspaceFile } from '../../api/workspaces'
import { workspaces } from '../../state/workspaceStore'
import styles from './FilePeek.module.css'

/**
 * The plain-text peek for a path named by a tool call.
 *
 * It read the session diff before S2 removed the review routes. With those
 * gone, there is no baseline to diff against, so this shows the file as it is
 * now, through `GET /workspaces/{id}/file`. Review belongs to the editor and
 * `git diff`.
 */
export function OpenedFile(props: { path: string | null; onClose: () => void }) {
  const [text, setText] = createSignal<string | null>(null)
  const [failed, setFailed] = createSignal<string | null>(null)

  createEffect(() => {
    const path = props.path
    setText(null)
    setFailed(null)
    if (!path) {
      return
    }
    const workspaceId = workspaces.activeWorkspaceId
    if (!workspaceId) {
      setFailed('Open a workspace to see this file.')
      return
    }
    let cancelled = false
    void readWorkspaceFile(workspaceId, path)
      .then((content) => {
        if (!cancelled) {
          setText(content)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setFailed(err instanceof Error ? err.message : 'Could not read this file.')
        }
      })
    onCleanup(() => {
      cancelled = true
    })
  })

  return (
    <Show when={props.path}>
      <Dialog
        open={props.path !== null}
        onOpenChange={(next) => {
          if (!next) {
            props.onClose()
          }
        }}
      >
        <Dialog.Portal>
          <Dialog.Overlay class={styles.overlay} />
          <Dialog.Content class={styles.panel}>
            <div class={styles.head}>
              <Dialog.Title class={styles.title}>{props.path}</Dialog.Title>
              <Dialog.CloseButton class={styles.close}>Close</Dialog.CloseButton>
            </div>
            <Show
              when={text() !== null}
              fallback={<p class={styles.message}>{failed() ?? 'Loading…'}</p>}
            >
              <pre class={styles.body}>{text()}</pre>
            </Show>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog>
    </Show>
  )
}
