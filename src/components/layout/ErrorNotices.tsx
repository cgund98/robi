/** @jsxImportSource solid-js */
import { For, Show } from 'solid-js'

import { sessionDisplayTitle } from '../../api/sessions'
import styles from './AppLayout.module.css'
import { noticePlacement } from '../../state/errorLog'
import { chat } from '../../state/chatStore'
import { errors } from '../../state/errorLog'

export function ErrorNotices() {
  const shown = () => errors.entries.filter((entry) => noticePlacement(entry) === 'top')

  return (
    <Show when={shown().length > 0}>
      <div class={styles.noticeStack}>
        <For each={shown()}>
          {(entry) => {
            const session = () =>
              entry.sessionId
                ? chat.sessions.find((item) => item.id === entry.sessionId)
                : undefined
            const label = () => {
              const match = session()
              return match ? sessionDisplayTitle(match) : null
            }
            return (
              <div class={styles.banner} role="alert">
                <span>
                  <Show when={label()}>
                    {(text) => <span class={styles.noticeSession}>{text()}. </span>}
                  </Show>
                  {entry.message}
                </span>
                <button
                  type="button"
                  class={styles.bannerRetry}
                  onClick={() => errors.acknowledge(entry.id)}
                >
                  Dismiss
                </button>
              </div>
            )
          }}
        </For>
      </div>
    </Show>
  )
}
