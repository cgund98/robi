/** @jsxImportSource solid-js */
import { Popover } from '@kobalte/core/popover'
import { Show } from 'solid-js'

import styles from './Composer.module.css'
import { estimateContext, formatTokens, type ContextMessage } from './contextUsage'

const RADIUS = 6
const CIRCUMFERENCE = 2 * Math.PI * RADIUS

export function ContextMeter(props: {
  messages: ContextMessage[]
  draft: string
  pendingText: string
  contextWindow: number | null
  onCompact?: () => void
  compacting?: boolean
  compactDisabled?: boolean
}) {
  const estimate = () =>
    estimateContext(props.messages, props.draft, props.contextWindow, props.pendingText)
  const fill = () => {
    const percent = estimate().percent
    return percent == null ? 0 : percent / 100
  }
  const label = () => {
    const next = estimate()
    if (next.percent != null) {
      return `Context, ${next.percent} percent`
    }
    return next.lastTurn ? 'Context, window unknown' : 'Context, no usage yet'
  }

  return (
    <Popover>
      <Popover.Trigger class={styles.meter} aria-label={label()}>
        <svg class={styles.ring} viewBox="0 0 16 16" aria-hidden="true">
          <circle class={styles.track} cx="8" cy="8" r={RADIUS} />
          <circle
            class={styles.arc}
            cx="8"
            cy="8"
            r={RADIUS}
            stroke-dasharray={`${CIRCUMFERENCE} ${CIRCUMFERENCE}`}
            stroke-dashoffset={CIRCUMFERENCE * (1 - fill())}
            transform="rotate(-90 8 8)"
          />
        </svg>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class={styles.meterPanel}>
          <dl class={styles.meterList}>
            <div>
              <dt>Context</dt>
              <dd>{contextLine(estimate())}</dd>
            </div>
            <Show when={estimate().lastTurn}>
              {(usage) => (
                <div>
                  <dt>Last turn</dt>
                  <dd>{lastTurnLine(usage())}</dd>
                </div>
              )}
            </Show>
            <Show when={estimate().uncounted > 0}>
              <div>
                <dt>Not yet counted</dt>
                <dd>~{formatTokens(estimate().uncounted)} tokens</dd>
              </div>
            </Show>
          </dl>
          <Show when={props.onCompact}>
            <button
              type="button"
              class={styles.meterAction}
              disabled={props.compacting || props.compactDisabled}
              onClick={() => props.onCompact?.()}
            >
              {props.compacting ? 'Compacting…' : 'Compact'}
            </button>
          </Show>
        </Popover.Content>
      </Popover.Portal>
    </Popover>
  )
}

function contextLine(estimate: ReturnType<typeof estimateContext>): string {
  if (estimate.used == null) {
    return 'No usage yet'
  }
  if (estimate.window == null || estimate.percent == null) {
    return 'Window unknown'
  }
  return `${formatTokens(estimate.used)} / ${formatTokens(estimate.window)} · ${estimate.percent}%`
}

function lastTurnLine(usage: { input: number; output: number; cached: number }): string {
  const parts = [`${formatTokens(usage.input)} in`, `${formatTokens(usage.output)} out`]
  if (usage.cached > 0) {
    parts.push(`${formatTokens(usage.cached)} cached`)
  }
  return parts.join(', ')
}
