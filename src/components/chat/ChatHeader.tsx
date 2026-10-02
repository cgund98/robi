import styles from './ChatHeader.module.css'

type ChatHeaderProps = {
  workspace: string
  sessionTitle: string
}

export function ChatHeader({ workspace, sessionTitle }: ChatHeaderProps) {
  return (
    <header className={styles.header}>
      <span className={styles.workspace}>{workspace}</span>
      <span className={styles.sep} aria-hidden>
        /
      </span>
      <span className={styles.title}>{sessionTitle}</span>
      <span className={styles.chevron} aria-hidden>
        ▾
      </span>
    </header>
  )
}
