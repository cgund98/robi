/** @jsxImportSource solid-js */
import { Show } from 'solid-js'

import { ApiStatus } from '../layout/ApiStatus'
import { IndexStatusLine } from '../layout/IndexStatusLine'
import { McpTray } from '../layout/McpTray'
import styles from '../../../components/chat/ChatHeader.module.css'

export function ChatHeader(props: { sessionTitle: string }) {
  return (
    <header class={styles.header} data-tauri-drag-region="deep">
      <div class={styles.leading}>
        <IndexStatusLine />
      </div>
      <Show when={props.sessionTitle}>
        <span class={styles.title}>{props.sessionTitle}</span>
      </Show>
      <div class={styles.actions}>
        <McpTray />
        <ApiStatus />
      </div>
    </header>
  )
}
