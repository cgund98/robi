import styles from './ChatHeader.module.css'

type ChatHeaderProps = {
  sessionTitle: string
}

export function ChatHeader({ sessionTitle }: ChatHeaderProps) {
  return (
    <header className={styles.header} data-tauri-drag-region="deep">
      <span className={styles.title}>{sessionTitle}</span>
    </header>
  )
}
