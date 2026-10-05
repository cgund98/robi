import { useEffect, useState } from 'react'

import { getSettings, putSetting, SETTING_KEYS } from '../../api/settings'
import styles from './Settings.module.css'

export function PermissionsSettings() {
  const [searchApproval, setSearchApproval] = useState(true)
  const [fetchApproval, setFetchApproval] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [search, fetch] = await getSettings([
          SETTING_KEYS.webSearchApproval,
          SETTING_KEYS.webFetchApproval
        ])
        if (!cancelled) {
          setSearchApproval(search.value !== 'off')
          setFetchApproval(fetch.value !== 'off')
        }
      } catch (err: unknown) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load settings')
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  async function toggle(key: string, current: boolean, apply: (next: boolean) => void) {
    const next = !current
    apply(next)
    setError(null)
    try {
      await putSetting(key, next ? 'on' : 'off', false)
    } catch (err: unknown) {
      apply(!next)
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  return (
    <>
      <h1 className={styles.title}>Permissions</h1>
      <div className={styles.card}>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Web search approval</div>
            <div className={styles.hint}>
              Ask before every web search. Each search spends Brave quota. The next turn uses this.
            </div>
          </div>
          <button
            type="button"
            className={styles.switch}
            role="switch"
            aria-checked={searchApproval}
            aria-label="Web search approval"
            onClick={() => {
              void toggle(SETTING_KEYS.webSearchApproval, searchApproval, setSearchApproval)
            }}
          >
            <span className={styles.knob} />
          </button>
        </div>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Web fetch approval</div>
            <div className={styles.hint}>
              Ask the first time a host is fetched in a session. Later calls to that host stay
              allowed. The next turn uses this.
            </div>
          </div>
          <button
            type="button"
            className={styles.switch}
            role="switch"
            aria-checked={fetchApproval}
            aria-label="Web fetch approval"
            onClick={() => {
              void toggle(SETTING_KEYS.webFetchApproval, fetchApproval, setFetchApproval)
            }}
          >
            <span className={styles.knob} />
          </button>
        </div>
      </div>
      {error ? <p className={`${styles.status} ${styles.statusError}`}>{error}</p> : null}
    </>
  )
}
