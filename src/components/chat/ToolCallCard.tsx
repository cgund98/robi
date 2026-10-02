import { useState } from 'react'

import type { AgentPhase } from '../../state/chatStore'
import styles from './ToolCallCard.module.css'
import {
  hasDetail,
  needsDecision,
  toolDetail,
  toolSummary,
  type ChatToolCall
} from './toolCallView'

type ToolCallCardProps = {
  call: ChatToolCall
  phase: AgentPhase
  busy: boolean
  onDecide: (decision: 'approve' | 'reject') => void
}

export function ToolCallCard({ call, phase, busy, onDecide }: ToolCallCardProps) {
  const summary = toolSummary(call)
  const decision = needsDecision(call, phase)
  const detail = toolDetail(call)
  const expandable = hasDetail(call) && !decision
  const [open, setOpen] = useState(false)

  if (decision) {
    return (
      <div className={styles.approval}>
        <p className={styles.approvalText}>
          <span className={styles.verb}>{summary.verb}</span>
          {summary.target ? <span className={styles.target}>{summary.target}</span> : null}
        </p>
        <div className={styles.actions}>
          <button
            type="button"
            className={styles.reject}
            disabled={busy}
            onClick={() => onDecide('reject')}
          >
            Reject
          </button>
          <button
            type="button"
            className={styles.run}
            disabled={busy}
            onClick={() => onDecide('approve')}
          >
            Run
          </button>
        </div>
      </div>
    )
  }

  const status = statusOf(call, phase)

  return (
    <div className={styles.call}>
      <button
        type="button"
        className={`${styles.row} ${status === 'failed' ? styles.rowFailed : ''}`}
        aria-expanded={expandable ? open : undefined}
        aria-invalid={status === 'failed' ? true : undefined}
        disabled={!expandable}
        onClick={() => {
          if (expandable) {
            setOpen((current) => !current)
          }
        }}
      >
        <ToolIcon name={call.name} />
        <span className={styles.verb}>{summary.verb}</span>
        {summary.target ? <span className={styles.target}>{summary.target}</span> : null}
        <StatusMark status={status} />
      </button>
      {open && detail ? <Detail detail={detail} /> : null}
    </div>
  )
}

function statusOf(call: ChatToolCall, phase: AgentPhase): 'running' | 'failed' | null {
  if (
    call.execution_status === 'failed' ||
    call.execution_status === 'cancelled' ||
    call.execution_status === 'timed_out'
  ) {
    return 'failed'
  }
  if (call.execution_status === 'succeeded') {
    return null
  }
  if (phase !== 'idle' || call.execution_status === 'running') {
    return 'running'
  }
  return null
}

function StatusMark({ status }: { status: 'running' | 'failed' | null }) {
  if (status === 'running') {
    return <span className={styles.spinner} aria-label="Running" />
  }
  return null
}

function ToolIcon({ name }: { name: string }) {
  if (name === 'grep' || name === 'find') {
    return (
      <svg className={styles.icon} viewBox="0 0 16 16" aria-hidden>
        <circle cx="7" cy="7" r="4.2" fill="none" stroke="currentColor" strokeWidth="1.4" />
        <path
          d="M10.2 10.2 13.2 13.2"
          stroke="currentColor"
          strokeWidth="1.4"
          strokeLinecap="round"
        />
      </svg>
    )
  }
  return (
    <svg className={styles.icon} viewBox="0 0 16 16" aria-hidden>
      <path
        d="M4 2.5h5.2L12.5 6v7.5h-8.5z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
      <path d="M9 2.7V6h3.2" fill="none" stroke="currentColor" strokeWidth="1.3" />
    </svg>
  )
}

function Detail({ detail }: { detail: NonNullable<ReturnType<typeof toolDetail>> }) {
  if (detail.kind === 'error') {
    return <pre className={styles.error}>{detail.text}</pre>
  }
  if (detail.kind === 'code') {
    return (
      <pre className={styles.detail}>
        {detail.lines.map((line, index) => (
          <span key={index} className={styles.codeLine}>
            <span className={styles.lineNo}>{detail.startLine + index}</span>
            <span>{line}</span>
          </span>
        ))}
      </pre>
    )
  }
  return (
    <pre className={styles.detail}>
      {detail.lines.map((line, index) => (
        <span key={index} className={styles.plainLine}>
          {line}
        </span>
      ))}
    </pre>
  )
}
