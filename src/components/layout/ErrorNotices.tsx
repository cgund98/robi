import { sessionDisplayTitle } from '../../api/sessions'
import { noticePlacement, useErrorLog } from '../../state/errorLog'
import { useChatStore } from '../../state/chatStore'
import styles from './AppLayout.module.css'

export function ErrorNotices() {
  const entries = useErrorLog((state) => state.entries)
  const acknowledge = useErrorLog((state) => state.acknowledge)
  const sessions = useChatStore((state) => state.sessions)
  const shown = entries.filter((entry) => noticePlacement(entry) === 'top')
  if (shown.length === 0) {
    return null
  }

  return (
    <div className={styles.noticeStack}>
      {shown.map((entry) => {
        const session = entry.sessionId
          ? sessions.find((item) => item.id === entry.sessionId)
          : undefined
        const label = session ? sessionDisplayTitle(session) : null
        return (
          <div key={entry.id} className={styles.banner} role="alert">
            <span>
              {label ? <span className={styles.noticeSession}>{label}. </span> : null}
              {entry.message}
            </span>
            <button
              type="button"
              className={styles.bannerRetry}
              onClick={() => acknowledge(entry.id)}
            >
              Dismiss
            </button>
          </div>
        )
      })}
    </div>
  )
}
