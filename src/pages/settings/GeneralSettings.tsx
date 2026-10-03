import { useEffect, useState } from 'react'

import { getSetting, putSetting, SETTING_KEYS } from '../../api/settings'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './Settings.module.css'

export function GeneralSettings() {
  const workspaces = useWorkspaceStore((state) => state.workspaces)
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const loaded = useWorkspaceStore((state) => state.loaded)
  const loadWorkspaces = useWorkspaceStore((state) => state.loadWorkspaces)
  const active = workspaces.find((workspace) => workspace.id === activeWorkspaceId) ?? null
  const [lsp, setLsp] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!loaded) {
      void loadWorkspaces()
    }
  }, [loaded, loadWorkspaces])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const setting = await getSetting(SETTING_KEYS.lsp)
        if (!cancelled) {
          setLsp(setting.value !== 'off')
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

  async function toggleLsp() {
    const next = !lsp
    setLsp(next)
    setError(null)
    try {
      await putSetting(SETTING_KEYS.lsp, next ? 'on' : 'off', false)
    } catch (err: unknown) {
      setLsp(!next)
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  return (
    <>
      <h1 className={styles.title}>General</h1>
      <div className={styles.card}>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Workspace</div>
            <div className={styles.hint}>
              {active ? active.root : 'No workspace selected. Open one from Workspaces.'}
            </div>
          </div>
          <input
            className={`${styles.input} ${styles.control}`}
            value={active?.name ?? ''}
            readOnly
            aria-label="Workspace"
          />
        </div>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Language server</div>
            <div className={styles.hint}>
              Diagnostics, definition, references, hover, and workspace symbols. The next turn uses
              this.
            </div>
          </div>
          <button
            type="button"
            className={styles.switch}
            role="switch"
            aria-checked={lsp}
            aria-label="Language server"
            onClick={() => {
              void toggleLsp()
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
