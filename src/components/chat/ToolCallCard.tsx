import { useEffect, useState, type ReactNode } from 'react'

import { getToolOriginal } from '../../api/messages'
import { useChatStore, type AgentPhase } from '../../state/chatStore'
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
  type RetrieveView,
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
          originalId={call.original_id}
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
        originalId={call.original_id}
        status={<StatusMark status={status} />}
      />
    )
  }

  if (detail?.kind === 'retrieve') {
    return (
      <RetrieveCard summary={summary} view={detail.view} status={<StatusMark status={status} />} />
    )
  }

  if (detail?.kind === 'mcp') {
    return (
      <McpCard
        key="mcp-result"
        summary={summary}
        args={detail.args}
        output={detail.output}
        originalId={call.original_id}
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
  if (name === 'retrieve') {
    return (
      <svg className={styles.icon} viewBox="0 0 16 16" aria-hidden>
        <path
          d="M3 4.5h10M3 8h10M3 11.5h6"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.4"
          strokeLinecap="round"
        />
      </svg>
    )
  }
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
  originalId,
  actions,
  status,
  defaultOpen = false
}: {
  summary: { verb: string; target: string }
  command: string
  output: string
  originalId?: string | null
  actions?: ReactNode
  status?: ReactNode
  defaultOpen?: boolean
}) {
  const sessionId = useChatStore((state) => state.activeSessionId)
  const shown = useCappedOutput(sessionId, originalId, output)
  const [open, setOpen] = useState(defaultOpen)
  const [openedForApproval, setOpenedForApproval] = useState(defaultOpen)
  if (defaultOpen && !openedForApproval) {
    setOpenedForApproval(true)
    setOpen(true)
  }
  const header = (
    <div className={styles.editHead}>
      <button
        type="button"
        className={open ? styles.editRow : styles.row}
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
  )
  if (!open) {
    return header
  }
  return (
    <div className={styles.shell}>
      {header}
      <ShellBody command={command} output={shown} />
    </div>
  )
}

function useCappedOutput(
  sessionId: string | null,
  originalId: string | null | undefined,
  fallback: string
): string {
  const [loaded, setLoaded] = useState<{ id: string; text: string } | null>(null)
  useEffect(() => {
    if (!sessionId || !originalId) {
      return
    }
    let cancelled = false
    const id = originalId
    getToolOriginal(sessionId, id)
      .then((body) => {
        if (cancelled) {
          return
        }
        const text =
          body.kind === 'mcp'
            ? body.text
            : [body.stdout, body.stderr].filter((part) => part.length > 0).join('\n')
        if (text.length > 0) {
          setLoaded({ id, text })
        }
      })
      .catch(() => {
        // A missing row shows the transcript text.
      })
    return () => {
      cancelled = true
    }
  }, [sessionId, originalId])
  if (loaded && originalId && loaded.id === originalId) {
    return loaded.text
  }
  return fallback
}

function McpCard({
  summary,
  args,
  output,
  originalId,
  actions,
  status,
  defaultOpen = false
}: {
  summary: { verb: string; target: string }
  args: string
  output: string
  originalId?: string | null
  actions?: ReactNode
  status?: ReactNode
  defaultOpen?: boolean
}) {
  const sessionId = useChatStore((state) => state.activeSessionId)
  const shown = useCappedOutput(sessionId, originalId, output)
  const [open, setOpen] = useState(defaultOpen)
  const [openedForApproval, setOpenedForApproval] = useState(defaultOpen)
  if (defaultOpen && !openedForApproval) {
    setOpenedForApproval(true)
    setOpen(true)
  }
  const header = (
    <div className={styles.editHead}>
      <button
        type="button"
        className={open ? styles.editRow : styles.row}
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        <span className={styles.verb}>{summary.verb}</span>
        <SummaryLabel summary={summary} />
        {status}
      </button>
      {actions}
    </div>
  )
  if (!open) {
    return header
  }
  return (
    <div className={styles.shell}>
      {header}
      <pre className={styles.shellBody}>
        {args}
        {shown ? `\n${shown}` : ''}
      </pre>
    </div>
  )
}

function RetrieveCard({
  summary,
  view,
  status
}: {
  summary: ToolSummary
  view: RetrieveView
  status?: ReactNode
}) {
  const [open, setOpen] = useState(false)
  const page =
    view.totalLines > 0
      ? `lines ${view.startLine}–${view.endLine || view.startLine} of ${view.totalLines}`
      : null
  const facts = [
    view.stream === 'both' ? 'stdout and stderr' : view.stream,
    view.exitCode != null ? `exit ${view.exitCode}` : null,
    view.truncated ? 'truncated' : null,
    view.raw ? 'raw' : null,
    page,
    view.nextOffset != null ? `more from L${view.nextOffset}` : null
  ].filter((fact): fact is string => fact != null && fact.length > 0)
  const header = (
    <button
      type="button"
      className={open ? styles.editRow : styles.row}
      aria-expanded={open}
      onClick={() => setOpen((current) => !current)}
    >
      <ToolIcon name="retrieve" />
      <span className={styles.verb}>{summary.verb}</span>
      <SummaryLabel summary={summary} />
      {status}
    </button>
  )
  if (!open) {
    return header
  }
  return (
    <div className={styles.shell}>
      {header}
      <div className={styles.retrieve}>
        <p className={styles.retrieveMeta}>{facts.join(' · ')}</p>
        {view.stdout ? <LogLines text={view.stdout} startLine={view.startLine} /> : null}
        {view.stderr ? (
          <>
            <p className={styles.retrieveStream}>stderr</p>
            <LogLines text={view.stderr} startLine={view.stream === 'stderr' ? view.startLine : 1} />
          </>
        ) : null}
        {!view.stdout && !view.stderr ? <p className={styles.retrieveMeta}>Empty page</p> : null}
      </div>
    </div>
  )
}

function LogLines({ text, startLine }: { text: string; startLine: number }) {
  const lines = text.split('\n')
  return (
    <div className={styles.retrieveLog}>
      {lines.map((line, index) => (
        <span
          key={index}
          className={line.startsWith('<<<ROBI_') ? styles.markerLine : styles.codeLine}
        >
          <span className={styles.lineNo}>{startLine + index}</span>
          <span>{line}</span>
        </span>
      ))}
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
  detail: Exclude<
    NonNullable<ReturnType<typeof toolDetail>>,
    { kind: 'shell' } | { kind: 'mcp' } | { kind: 'retrieve' }
  >
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
