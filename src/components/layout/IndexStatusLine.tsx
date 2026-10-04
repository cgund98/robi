import { memo, useEffect, useState } from 'react'

import { useIndexStore } from '../../state/indexStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './Sidebar.module.css'

export function IndexStatusLine() {
  const workspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const status = useIndexStore((state) => (state.workspaceId === workspaceId ? state.status : null))
  const pending = useIndexStore((state) => state.pending)
  const setPaused = useIndexStore((state) => state.setPaused)

  const indexing = status?.state === 'indexing'
  const [indexingVisible, setIndexingVisible] = useState(false)
  const [wasIndexing, setWasIndexing] = useState(indexing)
  if (wasIndexing !== indexing) {
    setWasIndexing(indexing)
    if (!indexing) {
      setIndexingVisible(false)
    }
  }

  useEffect(() => {
    if (!indexing) {
      return
    }
    const timer = window.setTimeout(() => setIndexingVisible(true), 5000)
    return () => window.clearTimeout(timer)
  }, [indexing])

  if (!status || status.state === 'ready' || (indexing && !indexingVisible)) {
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
        {busy || pending ? <IndexSpinner /> : null}
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

const IndexSpinner = memo(function IndexSpinner() {
  return <span className={styles.spinner} aria-hidden />
})

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
