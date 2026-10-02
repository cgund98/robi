import { useEffect } from 'react'

import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './Settings.module.css'

export function GeneralSettings() {
  const workspaces = useWorkspaceStore((state) => state.workspaces)
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const loaded = useWorkspaceStore((state) => state.loaded)
  const loadWorkspaces = useWorkspaceStore((state) => state.loadWorkspaces)
  const active = workspaces.find((workspace) => workspace.id === activeWorkspaceId) ?? null

  useEffect(() => {
    if (!loaded) {
      void loadWorkspaces()
    }
  }, [loaded, loadWorkspaces])

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
      </div>
    </>
  )
}
