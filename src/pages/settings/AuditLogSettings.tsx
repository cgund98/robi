import { useState } from 'react'

import { sessionDisplayTitle } from '../../api/sessions'
import { useChatStore } from '../../state/chatStore'
import { useErrorLog } from '../../state/errorLog'
import { useRequestLog, type RequestEntry } from '../../state/requestLog'
import styles from './Settings.module.css'

function clock(at: number): string {
  return new Date(at).toLocaleTimeString([], {
    hour: 'numeric',
    minute: '2-digit',
    second: '2-digit'
  })
}

const PAGE_SIZE = 50

function responseText(body: string): string {
  if (!body) {
    return '(empty)'
  }
  try {
    return JSON.stringify(JSON.parse(body), null, 2)
  } catch {
    return body
  }
}

function RequestRows({
  row,
  open,
  onToggle
}: {
  row: RequestEntry
  open: boolean
  onToggle: () => void
}) {
  return (
    <>
      <tr className={styles.requestRow} onClick={onToggle}>
        <td>{clock(row.at)}</td>
        <td>{row.method}</td>
        <td className={styles.requestPath}>{row.path}</td>
        <td>{row.status ?? '—'}</td>
        <td>{row.durationMs} ms</td>
      </tr>
      {open ? (
        <tr>
          <td colSpan={5} className={styles.requestBodyCell}>
            <pre className={styles.requestBody}>{responseText(row.body)}</pre>
          </td>
        </tr>
      ) : null}
    </>
  )
}

export function AuditLogSettings() {
  const entries = useErrorLog((state) => state.entries)
  const requests = useRequestLog((state) => state.entries)
  const sessions = useChatStore((state) => state.sessions)
  const [pageIndex, setPageIndex] = useState(0)
  const [openId, setOpenId] = useState<string | null>(null)
  const pageCount = Math.max(1, Math.ceil(requests.length / PAGE_SIZE))
  const page = Math.min(pageIndex, pageCount - 1)
  const pageRows = requests.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE)

  return (
    <>
      <h1 className={styles.title}>Audit log</h1>
      <p className={styles.lead}>
        Errors from this run of the app. The list is cleared when Robi restarts.
      </p>
      <h2 className={styles.sectionTitle}>Requests</h2>
      <p className={styles.sectionHint}>
        The last 250 API calls, newest first, 50 at a time. Health checks are left out.
      </p>
      {requests.length === 0 ? (
        <p className={styles.sectionHint}>No requests yet.</p>
      ) : (
        <div className={styles.card}>
          <table className={styles.requestTable}>
            <thead>
              <tr>
                <th>Time</th>
                <th>Method</th>
                <th>Path</th>
                <th>Status</th>
                <th>Duration</th>
              </tr>
            </thead>
            <tbody>
              {pageRows.map((row) => (
                <RequestRows
                  key={row.id}
                  row={row}
                  open={openId === row.id}
                  onToggle={() => setOpenId(openId === row.id ? null : row.id)}
                />
              ))}
            </tbody>
          </table>
          {pageCount > 1 ? (
            <div className={styles.requestPager}>
              <button
                type="button"
                className={styles.requestPageButton}
                disabled={page === 0}
                onClick={() => setPageIndex(page - 1)}
              >
                Previous
              </button>
              <span>
                {page * PAGE_SIZE + 1}–{page * PAGE_SIZE + pageRows.length} of {requests.length}
              </span>
              <button
                type="button"
                className={styles.requestPageButton}
                disabled={page >= pageCount - 1}
                onClick={() => setPageIndex(page + 1)}
              >
                Next
              </button>
            </div>
          ) : null}
        </div>
      )}
      <h2 className={styles.sectionTitle}>Errors</h2>
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
                ? 'Deleted session'
                : 'App'
            const when = clock(entry.at)
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
