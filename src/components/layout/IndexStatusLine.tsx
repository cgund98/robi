import { useEffect } from 'react'

import { useIndexStore } from '../../state/indexStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './Sidebar.module.css'

export function IndexStatusLine() {
  const workspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const status = useIndexStore((state) => (state.workspaceId === workspaceId ? state.status : null))
  const pending = useIndexStore((state) => state.pending)
  const refresh = useIndexStore((state) => state.refresh)
  const setPaused = useIndexStore((state) => state.setPaused)

  useEffect(() => {
    if (!workspaceId) {
      return
    }
    void refresh(workspaceId)
  }, [workspaceId, refresh])

  if (!status || status.state === 'ready') {
    return null
  }

  const label = lineLabel(status.state, status.files_done, status.files_total)
  const busy = status.state === 'downloading' || status.state === 'indexing'
  const title = status.state === 'failed' ? (status.error ?? 'Index failed') : undefined
  const control =
    pending === 'pause' ? 'Pausing' : pending === 'resume' ? 'Resuming' : busy ? 'Pause' : 'Resume'

  return (
    <>
      <div className={styles.indexLine} title={title} aria-busy={pending ? true : undefined}>
        {busy || pending ? <span className={styles.spinner} aria-hidden /> : null}
        <span className={styles.indexText}>{label}</span>
        <button
          type="button"
          className={styles.indexButton}
          disabled={pending !== null}
          onClick={() => {
            void setPaused(busy)
          }}
        >
          {control}
        </button>
      </div>
      <div className={styles.indexDivider} role="separator" />
    </>
  )
}

function lineLabel(state: string, done: number, total: number): string {
  switch (state) {
    case 'downloading':
      return 'Downloading index'
    case 'indexing':
      return `Indexing ${done}/${total}`
    case 'paused':
      return 'Index paused'
    case 'failed':
      return 'Index failed'
    default:
      return ''
  }
}
