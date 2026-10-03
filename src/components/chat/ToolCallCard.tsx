import { useState, type ReactNode } from 'react'

import type { AgentPhase } from '../../state/chatStore'
import styles from './ToolCallCard.module.css'
import {
  EDIT_VISIBLE_LINES,
  editPreview,
  editSuggestion,
  exploreSummary,
  hasDetail,
  needsDecision,
  planView,
  subagentCount,
  subagentStepSummary,
  subagentView,
  toolDetail,
  toolSummary,
  type ChatToolCall,
  type ToolSummary,
  type DiffLine,
  type EditPreview,
  type PlanView,
  type SubagentStepView
} from './toolCallView'

type ToolCallCardProps = {
  call: ChatToolCall
  phase: AgentPhase
  busy: boolean
  buildDisabled?: boolean
  onDecide: (decision: 'approve' | 'reject') => void
  onBuild?: (path: string) => void
  onViewPlan?: (plan: PlanView) => void
}

export function ToolCallCard({
  call,
  phase,
  busy,
  buildDisabled = false,
  onDecide,
  onBuild,
  onViewPlan
}: ToolCallCardProps) {
  const summary = toolSummary(call)
  const decision = needsDecision(call, phase)
  const preview = editPreview(call)
  const detail = toolDetail(call)
  const expandable = (preview != null || hasDetail(call) || detail?.kind === 'shell') && !decision
  const [open, setOpen] = useState(false)

  if (decision) {
    const suggestion = editSuggestion(call)
    const actions = <ApprovalActions busy={busy} onDecide={onDecide} />
    if (suggestion) {
      return <EditDiff preview={suggestion} actions={actions} />
    }
    if (detail?.kind === 'shell') {
      return (
        <ShellCard
          summary={summary}
          command={detail.command}
          output={detail.output}
          actions={actions}
          defaultOpen
        />
      )
    }
    if (detail?.kind === 'mcp') {
      return (
        <McpCard
          key="mcp-approval"
          summary={summary}
          args={detail.args}
          output={detail.output}
          actions={actions}
          defaultOpen
        />
      )
    }
    if (call.name === 'web_search' || call.name === 'web_fetch') {
      const headline = call.name === 'web_search' ? 'the web' : fetchHost(summary.target)
      const detail =
        summary.target && summary.target !== 'the web' && summary.target !== 'a page'
          ? summary.target
          : null
      return (
        <div className={styles.webApproval}>
          <div className={styles.webApprovalBody}>
            <p className={styles.approvalText}>
              <ToolIcon name={call.name} />
              <span className={styles.verb}>{summary.verb}</span>
              <span className={styles.target}>{headline}</span>
            </p>
            {detail ? <p className={styles.webDetail}>{detail}</p> : null}
          </div>
          {actions}
        </div>
      )
    }
    return (
      <div className={styles.approval}>
        <p className={styles.approvalText}>
          <span className={styles.verb}>{summary.verb}</span>
          <SummaryLabel summary={summary} />
        </p>
        {actions}
      </div>
    )
  }

  const status = statusOf(call, phase)

  if (call.name === 'delegate') {
    return <DelegateCard call={call} status={status} />
  }

  const saved = planView(call)
  if (saved) {
    return (
      <PlanCard
        plan={saved}
        buildDisabled={buildDisabled || saved.path.length === 0}
        onBuild={() => {
          if (saved.path && onBuild) {
            onBuild(saved.path)
          }
        }}
        onView={() => onViewPlan?.(saved)}
      />
    )
  }

  if (preview && status !== 'failed') {
    return <EditDiff preview={preview} />
  }

  if (detail?.kind === 'shell') {
    return (
      <ShellCard
        summary={summary}
        command={detail.command}
        output={detail.output}
        status={<StatusMark status={status} />}
      />
    )
  }

  if (detail?.kind === 'mcp') {
    return (
      <McpCard
        key="mcp-result"
        summary={summary}
        args={detail.args}
        output={detail.output}
        status={<StatusMark status={status} />}
        defaultOpen={false}
      />
    )
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
        <SummaryLabel summary={summary} />
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

function PlanCard({
  plan,
  buildDisabled,
  onBuild,
  onView
}: {
  plan: PlanView
  buildDisabled: boolean
  onBuild: () => void
  onView: () => void
}) {
  return (
    <div className={styles.planCard}>
      <p className={styles.planStatus}>{plan.created ? 'Created Plan' : 'Updated Plan'}</p>
      <p className={styles.planTitle}>{plan.title}</p>
      {plan.summary ? <p className={styles.planSummary}>{plan.summary}</p> : null}
      <div className={styles.planActions}>
        <button type="button" className={styles.viewPlan} onClick={onView}>
          View Plan
        </button>
        <button
          type="button"
          className={styles.planBuild}
          disabled={buildDisabled}
          onClick={onBuild}
        >
          Build
        </button>
      </div>
    </div>
  )
}

function DelegateCard({
  call,
  status
}: {
  call: ChatToolCall
  status: 'running' | 'failed' | null
}) {
  const view = subagentView(call)
  const summary = toolSummary(call)
  const mode = view?.mode ?? 'explore'
  const label = mode === 'explore' ? 'Explore' : 'General'
  const steps = view?.steps ?? []
  const answer = view?.answer ?? ''
  const [open, setOpen] = useState(false)
  const [answerOpen, setAnswerOpen] = useState(false)
  const title = view?.description || summary.target

  return (
    <div className={styles.call}>
      <button
        type="button"
        className={`${styles.row} ${status === 'failed' ? styles.rowFailed : ''}`}
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        {mode === 'explore' ? (
          <span className={styles.exploreSummary}>{view ? exploreSummary(view) : 'Exploring'}</span>
        ) : (
          <>
            <span className={styles.verb}>{label}</span>
            <StatusMark status={status} />
          </>
        )}
      </button>
      {open ? (
        <div className={styles.delegate}>
          {title ? <p className={styles.delegateTitle}>{title}</p> : null}
          {view && mode !== 'explore' ? <p className={styles.meta}>{subagentCount(view)}</p> : null}
          {steps.length > 0 ? (
            <ul className={styles.steps}>
              {steps.map((step, index) => (
                <StepRow key={`${step.name}-${index}`} step={step} />
              ))}
            </ul>
          ) : null}
          {answer ? (
            <button
              type="button"
              className={styles.answerToggle}
              aria-expanded={answerOpen}
              onClick={() => setAnswerOpen((current) => !current)}
            >
              Answer
            </button>
          ) : null}
          {answerOpen && answer ? <pre className={styles.detail}>{answer}</pre> : null}
        </div>
      ) : null}
    </div>
  )
}

function StepRow({ step }: { step: SubagentStepView }) {
  const summary = subagentStepSummary(step)
  const failed = step.status === 'denied' || step.status === 'failed'
  return (
    <li className={`${styles.step} ${failed ? styles.rowFailed : ''}`}>
      <ToolIcon name={step.name} />
      <span className={styles.verb}>{summary.verb}</span>
      <SummaryLabel summary={summary} />
      {step.status === 'running' ? <span className={styles.spinner} aria-label="Running" /> : null}
    </li>
  )
}

function SummaryLabel({ summary }: { summary: ToolSummary }) {
  if (!summary.target && !summary.range) {
    return null
  }
  return (
    <>
      {summary.target ? <span className={styles.target}>{summary.target}</span> : null}
      {summary.range ? <span className={styles.range}>{summary.range}</span> : null}
    </>
  )
}

function StatusMark({ status }: { status: 'running' | 'failed' | null }) {
  if (status === 'running') {
    return <span className={styles.spinner} aria-label="Running" />
  }
  return null
}

function fetchHost(url: string): string {
  try {
    return new URL(url).host || 'a page'
  } catch {
    return 'a page'
  }
}

function ToolIcon({ name }: { name: string }) {
  if (name === 'shell') {
    return (
      <svg className={styles.icon} viewBox="0 0 16 16" aria-hidden>
        <path
          d="M3 4.5 6.8 8 3 11.5"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <path
          d="M8.5 12.5h5"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
          strokeLinecap="round"
        />
      </svg>
    )
  }
  if (name === 'grep' || name === 'find' || name === 'web_search') {
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

function ShellCard({
  summary,
  command,
  output,
  actions,
  status,
  defaultOpen = false
}: {
  summary: { verb: string; target: string }
  command: string
  output: string
  actions?: ReactNode
  status?: ReactNode
  defaultOpen?: boolean
}) {
  const [open, setOpen] = useState(defaultOpen)
  const [openedForApproval, setOpenedForApproval] = useState(defaultOpen)
  if (defaultOpen && !openedForApproval) {
    setOpenedForApproval(true)
    setOpen(true)
  }
  const preview = outputPreview(output, 4)
  return (
    <div className={styles.shell}>
      <div className={styles.editHead}>
        <button
          type="button"
          className={styles.editRow}
          aria-expanded={open}
          onClick={() => setOpen((current) => !current)}
        >
          <ToolIcon name="shell" />
          <span className={styles.verb}>{summary.verb}</span>
          <SummaryLabel summary={summary} />
          {status}
        </button>
        {actions}
      </div>
      {open ? <ShellBody command={command} output={output} /> : null}
      {!open && preview.text ? (
        <pre
          className={`${styles.shellBody} ${styles.shellPreview}`}
          data-more-above={preview.moreAbove}
          data-more-below={preview.moreBelow}
        >
          {preview.text}
        </pre>
      ) : null}
    </div>
  )
}

function outputPreview(
  output: string,
  count: number
): { text: string; moreAbove: boolean; moreBelow: boolean } {
  const lines = output.split('\n')
  if (lines[lines.length - 1] === '') {
    lines.pop()
  }
  const start = Math.max(0, lines.length - count)
  return {
    text: lines.slice(start).join('\n'),
    moreAbove: start > 0,
    moreBelow: false
  }
}

function McpCard({
  summary,
  args,
  output,
  actions,
  status,
  defaultOpen = false
}: {
  summary: { verb: string; target: string }
  args: string
  output: string
  actions?: ReactNode
  status?: ReactNode
  defaultOpen?: boolean
}) {
  const [open, setOpen] = useState(defaultOpen)
  const [openedForApproval, setOpenedForApproval] = useState(defaultOpen)
  if (defaultOpen && !openedForApproval) {
    setOpenedForApproval(true)
    setOpen(true)
  }
  return (
    <div className={styles.shell}>
      <div className={styles.editHead}>
        <button
          type="button"
          className={styles.editRow}
          aria-expanded={open}
          onClick={() => setOpen((current) => !current)}
        >
          <span className={styles.verb}>{summary.verb}</span>
          <SummaryLabel summary={summary} />
          {status}
        </button>
        {actions}
      </div>
      {open ? (
        <pre className={styles.shellBody}>
          {args}
          {output ? `\n${output}` : ''}
        </pre>
      ) : null}
    </div>
  )
}

function ShellBody({ command, output }: { command: string; output: string }) {
  return (
    <pre className={styles.shellBody}>
      <span className={styles.shellCommand}>
        <span className={styles.prompt}>$</span> {command}
      </span>
      {output}
    </pre>
  )
}

function Detail({
  detail
}: {
  detail: Exclude<NonNullable<ReturnType<typeof toolDetail>>, { kind: 'shell' } | { kind: 'mcp' }>
}) {
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
