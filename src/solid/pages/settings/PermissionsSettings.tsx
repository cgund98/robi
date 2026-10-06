/** @jsxImportSource solid-js */
import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

import { getSettings, putSetting, SETTING_KEYS } from '../../../api/settings'
import styles from '../../../pages/settings/Settings.module.css'

export function PermissionsSettings() {
  const [searchApproval, setSearchApproval] = createSignal(true)
  const [fetchApproval, setFetchApproval] = createSignal(true)
  const [error, setError] = createSignal<string | null>(null)

  createEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [search, fetchSetting] = await getSettings([
          SETTING_KEYS.webSearchApproval,
          SETTING_KEYS.webFetchApproval
        ])
        if (!cancelled) {
          setSearchApproval(search.value !== 'off')
          setFetchApproval(fetchSetting.value !== 'off')
        }
      } catch (err: unknown) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load settings')
        }
      }
    })()
    onCleanup(() => {
      cancelled = true
    })
  })

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
      <h1 class={styles.title}>Permissions</h1>
      <div class={styles.card}>
        <div class={styles.row}>
          <div class={styles.copy}>
            <div class={styles.label}>Web search approval</div>
            <div class={styles.hint}>
              Ask before every web search. Each search spends Brave quota. The next turn uses this.
            </div>
          </div>
          <button
            type="button"
            class={styles.switch}
            role="switch"
            aria-checked={searchApproval()}
            aria-label="Web search approval"
            onClick={() => {
              void toggle(SETTING_KEYS.webSearchApproval, searchApproval(), setSearchApproval)
            }}
          >
            <span class={styles.knob} />
          </button>
        </div>
        <div class={styles.row}>
          <div class={styles.copy}>
            <div class={styles.label}>Web fetch approval</div>
            <div class={styles.hint}>
              Ask the first time a host is fetched in a session. Later calls to that host stay
              allowed. The next turn uses this.
            </div>
          </div>
          <button
            type="button"
            class={styles.switch}
            role="switch"
            aria-checked={fetchApproval()}
            aria-label="Web fetch approval"
            onClick={() => {
              void toggle(SETTING_KEYS.webFetchApproval, fetchApproval(), setFetchApproval)
            }}
          >
            <span class={styles.knob} />
          </button>
        </div>
      </div>
      <Show when={error()}>
        <p class={`${styles.status} ${styles.statusError}`}>{error()}</p>
      </Show>
    </>
  )
}
