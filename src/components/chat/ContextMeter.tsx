import * as Popover from '@radix-ui/react-popover'

import styles from './Composer.module.css'
import { estimateContext, formatTokens, type ContextMessage } from './contextUsage'

const RADIUS = 6
const CIRCUMFERENCE = 2 * Math.PI * RADIUS

type ContextMeterProps = {
  messages: ContextMessage[]
  draft: string
  pendingText: string
  contextWindow: number | null
}

export function ContextMeter({
  messages,
  draft,
  pendingText,
  contextWindow
}: ContextMeterProps) {
  const estimate = estimateContext(messages, draft, contextWindow, pendingText)
  const percent = estimate.percent
  const fill = percent == null ? 0 : percent / 100
  const label =
    percent != null
      ? `Context, ${percent} percent`
      : estimate.lastTurn
        ? 'Context, window unknown'
        : 'Context, no usage yet'

  return (
    <Popover.Root>
      <Popover.Trigger className={styles.meter} aria-label={label}>
        <svg className={styles.ring} viewBox="0 0 16 16" aria-hidden>
          <circle className={styles.track} cx="8" cy="8" r={RADIUS} />
          <circle
            className={styles.arc}
            cx="8"
            cy="8"
            r={RADIUS}
            strokeDasharray={`${CIRCUMFERENCE} ${CIRCUMFERENCE}`}
            strokeDashoffset={CIRCUMFERENCE * (1 - fill)}
            transform="rotate(-90 8 8)"
          />
        </svg>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content className={styles.meterPanel} side="top" align="end" sideOffset={8}>
          <dl className={styles.meterList}>
            <div>
              <dt>Context</dt>
              <dd>{contextLine(estimate)}</dd>
            </div>
            {estimate.lastTurn ? (
              <div>
                <dt>Last turn</dt>
                <dd>{lastTurnLine(estimate.lastTurn)}</dd>
              </div>
            ) : null}
            {estimate.uncounted > 0 ? (
              <div>
                <dt>Not yet counted</dt>
                <dd>~{formatTokens(estimate.uncounted)} tokens</dd>
              </div>
            ) : null}
          </dl>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
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
