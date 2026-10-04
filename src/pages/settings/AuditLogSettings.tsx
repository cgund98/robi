import { sessionDisplayTitle } from '../../api/sessions'
import { useChatStore } from '../../state/chatStore'
import { useErrorLog } from '../../state/errorLog'
import styles from './Settings.module.css'

export function AuditLogSettings() {
  const entries = useErrorLog((state) => state.entries)
  const sessions = useChatStore((state) => state.sessions)

  return (
    <>
      <h1 className={styles.title}>Audit log</h1>
      <p className={styles.lead}>
        Errors from this run of the app. The list is cleared when Robi restarts.
      </p>
      {entries.length === 0 ? (
        <p className={styles.sectionHint}>No errors yet.</p>
      ) : (
        <div className={styles.card}>
          {entries.map((entry) => {
            const session = entry.sessionId
              ? sessions.find((item) => item.id === entry.sessionId)
              : undefined
            const where = session
              ? sessionDisplayTitle(session)
              : entry.sessionId
                ? 'Another chat'
                : 'App'
            const when = new Date(entry.at).toLocaleTimeString([], {
              hour: 'numeric',
              minute: '2-digit',
              second: '2-digit'
            })
            return (
              <div key={entry.id} className={styles.auditRow}>
                <div className={styles.auditMeta}>
                  <span>{where}</span>
                  <span>{when}</span>
                </div>
                <p className={styles.auditMessage}>{entry.message}</p>
              </div>
            )
          })}
        </div>
      )}
    </>
  )
}
