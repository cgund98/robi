import { useState, type ReactNode } from 'react'

import type { AgentPhase } from '../../state/chatStore'
import styles from './ToolCallCard.module.css'
import {
  EDIT_VISIBLE_LINES,
  editPreview,
  editSuggestion,
  hasDetail,
  needsDecision,
  toolDetail,
  toolSummary,
  type ChatToolCall,
  type DiffLine,
  type EditPreview
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
  const preview = editPreview(call)
  const detail = toolDetail(call)
  const expandable = (preview != null || hasDetail(call)) && !decision
  const [open, setOpen] = useState(false)

  if (decision) {
    const suggestion = editSuggestion(call)
    const actions = <ApprovalActions busy={busy} onDecide={onDecide} />
    if (suggestion) {
      return <EditDiff preview={suggestion} actions={actions} />
    }
    return (
      <div className={styles.approval}>
        <p className={styles.approvalText}>
          <span className={styles.verb}>{summary.verb}</span>
          {summary.target ? <span className={styles.target}>{summary.target}</span> : null}
        </p>
        {actions}
      </div>
    )
  }

  const status = statusOf(call, phase)

  if (preview && status !== 'failed') {
    return <EditDiff preview={preview} />
  }

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

function ApprovalActions({
  busy,
  onDecide
}: {
  busy: boolean
  onDecide: (decision: 'approve' | 'reject') => void
}) {
  return (
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
        className={styles.approve}
        disabled={busy}
        onClick={() => onDecide('approve')}
      >
        Approve
      </button>
    </div>
  )
}

function EditDiff({ preview, actions }: { preview: EditPreview; actions?: ReactNode }) {
  const [open, setOpen] = useState(false)
  const canExpand = preview.lines.length > EDIT_VISIBLE_LINES || preview.hidden > 0
  const visible = open ? preview.lines : preview.lines.slice(0, EDIT_VISIBLE_LINES)
  return (
    <div className={styles.edit}>
      <div className={styles.editHead}>
        <button
          type="button"
          className={styles.editRow}
          aria-expanded={canExpand ? open : undefined}
          disabled={!canExpand}
          onClick={() => {
            if (canExpand) {
              setOpen((current) => !current)
            }
          }}
        >
          <span className={styles.editPath} title={preview.path}>
            {preview.path}
          </span>
          <span className={styles.editCounts}>
            <span className={styles.add}>+{preview.additions}</span>
            <span className={styles.del}>-{preview.deletions}</span>
          </span>
        </button>
        {actions}
      </div>
      <DiffView lines={visible} hidden={open ? preview.hidden : 0} />
    </div>
  )
}

function DiffView({ lines, hidden }: { lines: DiffLine[]; hidden: number }) {
  return (
    <div className={styles.diff}>
      {lines.map((line, index) => (
        <div key={index} className={styles.diffLine} data-kind={line.kind}>
          <span className={styles.lineNo}>{line.lineNo}</span>
          <span className={styles.mark}>
            {line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' '}
          </span>
          <span className={styles.diffText}>{line.text}</span>
        </div>
      ))}
      {hidden > 0 ? <p className={styles.more}>{hidden} more lines</p> : null}
    </div>
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
