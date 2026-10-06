import { ApiStatus } from '../layout/ApiStatus'
import { IndexStatusLine } from '../layout/IndexStatusLine'
import { McpTray } from '../layout/McpTray'
import styles from './ChatHeader.module.css'

type ChatHeaderProps = {
  sessionTitle: string
}

export function ChatHeader({ sessionTitle }: ChatHeaderProps) {
  return (
    <header className={styles.header} data-tauri-drag-region="deep">
      <div className={styles.leading}>
        <IndexStatusLine />
      </div>
      {sessionTitle ? <span className={styles.title}>{sessionTitle}</span> : null}
      <div className={styles.actions}>
        <McpTray />
        <ApiStatus />
      </div>
    </header>
  )
}
