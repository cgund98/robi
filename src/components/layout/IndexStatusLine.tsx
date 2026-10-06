import { useEffect, useState } from 'react'
import * as Popover from '@radix-ui/react-popover'

import { useIndexStore } from '../../state/indexStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './IndexStatusLine.module.css'

const RADIUS = 6
const CIRCUMFERENCE = 2 * Math.PI * RADIUS

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

  const done = status.files_done
  const total = status.files_total
  const remaining = Math.max(0, total - done)
  const busy = status.state === 'downloading' || status.state === 'indexing'
  const known = total > 0
  const finishing = busy && known && remaining === 0
  const remainingFill = known ? remaining / total : 0
  const control =
    pending === 'pause' ? 'Pausing' : pending === 'resume' ? 'Resuming' : busy ? 'Pause' : 'Resume'
  const heading = menuHeading(status.state)
  const detail = indexDetail(status.state, done, total, remaining, finishing)
  const error = status.state === 'failed' ? status.error : null
  const summary = error ? `${heading}. ${error}.` : `${heading}.`

  return (
    <Popover.Root>
      <Popover.Trigger
        className={styles.wedge}
        aria-label={detail ? `${summary} ${detail}` : summary}
        aria-busy={pending ? true : undefined}
      >
        <ProgressWheel fill={remainingFill} indeterminate={!known && (busy || pending !== null)} />
        <span className={styles.label}>{wedgeLabel(status.state)}</span>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content className={styles.panel} side="bottom" align="start" sideOffset={8}>
          <p className={styles.heading}>{heading}</p>
          {error ? <p className={styles.error}>{error}</p> : null}
          {detail ? <p className={styles.progress}>{detail}</p> : null}
          <button
            type="button"
            className={styles.action}
            disabled={pending !== null}
            onClick={() => {
              void setPaused(busy)
            }}
          >
            {control}
          </button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

function ProgressWheel({ fill, indeterminate }: { fill: number; indeterminate: boolean }) {
  return (
    <svg
      className={indeterminate ? `${styles.wheel} ${styles.spin}` : styles.wheel}
      viewBox="0 0 16 16"
      aria-hidden
    >
      <circle className={styles.track} cx="8" cy="8" r={RADIUS} />
      <circle
        className={styles.arc}
        cx="8"
        cy="8"
        r={RADIUS}
        strokeDasharray={
          indeterminate
            ? `${CIRCUMFERENCE * 0.25} ${CIRCUMFERENCE}`
            : `${CIRCUMFERENCE} ${CIRCUMFERENCE}`
        }
        strokeDashoffset={indeterminate ? 0 : CIRCUMFERENCE * (1 - fill)}
        transform="rotate(-90 8 8)"
      />
    </svg>
  )
}

function wedgeLabel(state: string): string {
  switch (state) {
    case 'paused':
      return 'Paused'
    case 'failed':
      return 'Failed'
    default:
      return 'Indexing'
  }
}

function menuHeading(state: string): string {
  switch (state) {
    case 'downloading':
      return 'Downloading'
    case 'paused':
      return 'Paused'
    case 'failed':
      return 'Failed'
    default:
      return 'Indexing files for search'
  }
}

/**
 * The counts line. It stays off until the walk has seen a file, so the menu
 * never shows a meaningless `0/0` while the model downloads or the scan starts.
 */
function indexDetail(
  state: string,
  done: number,
  total: number,
  remaining: number,
  finishing: boolean
): string | null {
  if (total > 0) {
    return finishing ? 'Finishing up…' : `${done}/${total} · ${remaining} remaining`
  }
  switch (state) {
    case 'downloading':
      return 'Preparing the search model…'
    case 'indexing':
      return 'Scanning the workspace…'
    case 'paused':
      return 'Paused before the scan started'
    default:
      return null
  }
}
