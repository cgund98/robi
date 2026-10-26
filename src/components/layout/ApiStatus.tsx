import { onCleanup, onMount, Show } from 'solid-js'

import { apiHealthReason, watchApiHealth } from './apiHealth'
import styles from './ApiStatus.module.css'

/**
 * The API health mark: an exclamation beside the other chrome, and nothing
 * while the local API answers. `apiHealth.ts` owns the poll, so the top bar's
 * overflow can ask whether there is anything to show.
 */
export function ApiStatus() {
  onMount(() => {
    onCleanup(watchApiHealth())
  })

  return (
    <Show when={apiHealthReason()}>
      {(message) => (
        <span class={styles.mark} role="status" title={message()} aria-label={message()}>
          !
        </span>
      )}
    </Show>
  )
}
