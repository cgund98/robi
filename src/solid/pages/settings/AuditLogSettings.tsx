/** @jsxImportSource solid-js */
import { createSignal, For, Show } from 'solid-js'

import { sessionDisplayTitle } from '../../../api/sessions'
import styles from '../../../pages/settings/Settings.module.css'
import { type RequestEntry } from '../../../state/requestLog'
import { chat, errors, requests } from '../../state/host'

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

export function AuditLogSettings() {
  const [pageIndex, setPageIndex] = createSignal(0)
  const [openId, setOpenId] = createSignal<string | null>(null)
  const pageCount = () => Math.max(1, Math.ceil(requests.entries.length / PAGE_SIZE))
  const page = () => Math.min(pageIndex(), pageCount() - 1)
  const pageRows = () => requests.entries.slice(page() * PAGE_SIZE, (page() + 1) * PAGE_SIZE)

  return (
    <>
      <h1 class={styles.title}>Audit log</h1>
      <p class={styles.lead}>
        Errors from this run of the app. The list is cleared when Robi restarts.
      </p>
      <h2 class={styles.sectionTitle}>Requests</h2>
      <p class={styles.sectionHint}>
        The last 250 API calls, newest first, 50 at a time. Health checks are left out.
      </p>
      <Show
        when={requests.entries.length > 0}
        fallback={<p class={styles.sectionHint}>No requests yet.</p>}
      >
        <div class={styles.card}>
          <table class={styles.requestTable}>
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
              <For each={pageRows()}>
                {(row) => (
                  <RequestRows
                    row={row}
                    open={openId() === row.id}
                    onToggle={() => setOpenId(openId() === row.id ? null : row.id)}
                  />
                )}
              </For>
            </tbody>
          </table>
          <Show when={pageCount() > 1}>
            <div class={styles.requestPager}>
              <button
                type="button"
                class={styles.requestPageButton}
                disabled={page() === 0}
                onClick={() => setPageIndex(page() - 1)}
              >
                Previous
              </button>
              <span>
                {page() * PAGE_SIZE + 1}–{page() * PAGE_SIZE + pageRows().length} of{' '}
                {requests.entries.length}
              </span>
              <button
                type="button"
                class={styles.requestPageButton}
                disabled={page() >= pageCount() - 1}
                onClick={() => setPageIndex(page() + 1)}
              >
                Next
              </button>
            </div>
          </Show>
        </div>
      </Show>
      <h2 class={styles.sectionTitle}>Errors</h2>
      <Show
        when={errors.entries.length > 0}
        fallback={<p class={styles.sectionHint}>No errors yet.</p>}
      >
        <div class={styles.card}>
          <For each={errors.entries}>
            {(entry) => {
              const session = () =>
                entry.sessionId
                  ? chat.sessions.find((item) => item.id === entry.sessionId)
                  : undefined
              const where = () => {
                const current = session()
                if (current) {
                  return sessionDisplayTitle(current)
                }
                return entry.sessionId ? 'Deleted session' : 'App'
              }
              return (
                <div class={styles.auditRow}>
                  <div class={styles.auditMeta}>
                    <span>{where()}</span>
                    <span>{clock(entry.at)}</span>
                  </div>
                  <p class={styles.auditMessage}>{entry.message}</p>
                </div>
              )
            }}
          </For>
        </div>
      </Show>
    </>
  )
}

function RequestRows(props: { row: RequestEntry; open: boolean; onToggle: () => void }) {
  return (
    <>
      <tr class={styles.requestRow} onClick={() => props.onToggle()}>
        <td>{clock(props.row.at)}</td>
        <td>{props.row.method}</td>
        <td class={styles.requestPath}>{props.row.path}</td>
        <td>{props.row.status ?? '—'}</td>
        <td>{props.row.durationMs} ms</td>
      </tr>
      <Show when={props.open}>
        <tr>
          <td colspan={5} class={styles.requestBodyCell}>
            <pre class={styles.requestBody}>{responseText(props.row.body)}</pre>
          </td>
        </tr>
      </Show>
    </>
  )
}
