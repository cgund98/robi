/** @jsxImportSource solid-js */
import { Popover } from '@kobalte/core/popover'
import { createEffect, createMemo, createSignal, onCleanup, Show } from 'solid-js'

import type { IndexStatus } from '../../../api/codeIndex'
import styles from '../../../components/layout/IndexStatusLine.module.css'
import { index, workspaces } from '../../state/host'

const RADIUS = 6
const CIRCUMFERENCE = 2 * Math.PI * RADIUS

export function IndexStatusLine() {
  const status = () => (index.workspaceId === workspaces.activeWorkspaceId ? index.status : null)
  const indexing = () => status()?.state === 'indexing'
  const [indexingVisible, setIndexingVisible] = createSignal(false)

  createEffect(() => {
    if (indexing()) {
      return
    }
    setIndexingVisible(false)
  })

  createEffect(() => {
    if (!indexing()) {
      return
    }
    const timer = window.setTimeout(() => setIndexingVisible(true), 5000)
    onCleanup(() => window.clearTimeout(timer))
  })

  const view = createMemo((): IndexStatus | null => {
    const next = status()
    if (!next || next.state === 'ready' || (next.state === 'indexing' && !indexingVisible())) {
      return null
    }
    return next
  })

  return (
    <Show when={view()}>
      {(ready) => <IndexPopover status={ready()} pending={index.pending} />}
    </Show>
  )
}

function IndexPopover(props: { status: IndexStatus; pending: 'pause' | 'resume' | null }) {
  const done = () => props.status.files_done
  const total = () => props.status.files_total
  const remaining = () => Math.max(0, total() - done())
  const busy = () => props.status.state === 'downloading' || props.status.state === 'indexing'
  const known = () => total() > 0
  const finishing = () => busy() && known() && remaining() === 0
  const remainingFill = () => (known() ? remaining() / total() : 0)
  const control = () =>
    props.pending === 'pause'
      ? 'Pausing'
      : props.pending === 'resume'
        ? 'Resuming'
        : busy()
          ? 'Pause'
          : 'Resume'
  const heading = () => menuHeading(props.status.state)
  const detail = () => indexDetail(props.status.state, done(), total(), remaining(), finishing())
  const error = () => (props.status.state === 'failed' ? props.status.error : null)
  const summary = () => (error() ? `${heading()}. ${error()}.` : `${heading()}.`)

  return (
    <Popover>
      <Popover.Trigger
        class={styles.wedge}
        aria-label={detail() ? `${summary()} ${detail()}` : summary()}
        aria-busy={props.pending ? true : undefined}
      >
        <ProgressWheel
          fill={remainingFill()}
          indeterminate={!known() && (busy() || props.pending !== null)}
        />
        <span class={styles.label}>{wedgeLabel(props.status.state)}</span>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class={styles.panel}>
          <p class={styles.heading}>{heading()}</p>
          <Show when={error()}>{(message) => <p class={styles.error}>{message()}</p>}</Show>
          <Show when={detail()}>{(line) => <p class={styles.progress}>{line()}</p>}</Show>
          <button
            type="button"
            class={styles.action}
            disabled={props.pending !== null}
            onClick={() => {
              void index.setPaused(busy())
            }}
          >
            {control()}
          </button>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  )
}

function ProgressWheel(props: { fill: number; indeterminate: boolean }) {
  return (
    <svg
      class={props.indeterminate ? `${styles.wheel} ${styles.spin}` : styles.wheel}
      viewBox="0 0 16 16"
      aria-hidden="true"
    >
      <circle class={styles.track} cx="8" cy="8" r={RADIUS} />
      <circle
        class={styles.arc}
        cx="8"
        cy="8"
        r={RADIUS}
        stroke-dasharray={
          props.indeterminate
            ? `${CIRCUMFERENCE * 0.25} ${CIRCUMFERENCE}`
            : `${CIRCUMFERENCE} ${CIRCUMFERENCE}`
        }
        stroke-dashoffset={props.indeterminate ? 0 : CIRCUMFERENCE * (1 - props.fill)}
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
