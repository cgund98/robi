/** @jsxImportSource solid-js */
import { BarsBottomLeft, CommandLine, Document, MagnifyingGlass } from '../../components/ui/icons'
import { createEffect, createSignal, For, onCleanup, Show, type JSX } from 'solid-js'

import { getToolOriginal } from '../../api/messages'
import { type AgentPhase } from '../../state/chatStore'
import { chat } from '../../state/chatStore'
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
export function ToolCallCard(props: {
  call: ChatToolCall
  phase: AgentPhase
  busy: boolean
  buildDisabled?: boolean
  onDecide: (decision: 'approve' | 'reject') => void
  onBuild?: (path: string) => void
  onViewPlan?: (plan: PlanView) => void
}) {
  const summary = () => toolSummary(props.call)
  const decision = () => needsDecision(props.call, props.phase)
  const preview = () => editPreview(props.call)
  const detail = () => toolDetail(props.call)
  const expandable = () =>
    (preview() != null || hasDetail(props.call) || detail()?.kind === 'shell') && !decision()
  const [open, setOpen] = createSignal(false)
  const status = () => statusOf(props.call, props.phase)
  const shellDetail = () => {
    const next = detail()
    return next?.kind === 'shell' ? next : null
  }
  const mcpDetail = () => {
    const next = detail()
    return next?.kind === 'mcp' ? next : null
  }

  return (
    <Show
      when={!decision()}
      fallback={
        <ApprovalBody
          call={props.call}
          summary={summary()}
          detail={detail()}
          busy={props.busy}
          onDecide={props.onDecide}
        />
      }
    >
      <Show when={props.call.name === 'delegate'} fallback={null}>
        <DelegateCard call={props.call} status={status()} />
      </Show>
      <Show when={props.call.name !== 'delegate' && planView(props.call)}>
        {(saved) => (
          <PlanCard
            plan={saved()}
            buildDisabled={(props.buildDisabled ?? false) || saved().path.length === 0}
            onBuild={() => {
              if (saved().path && props.onBuild) {
                props.onBuild(saved().path)
              }
            }}
            onView={() => props.onViewPlan?.(saved())}
          />
        )}
      </Show>
      <Show
        when={
          props.call.name !== 'delegate' &&
          !planView(props.call) &&
          preview() &&
          status() !== 'failed'
        }
      >
        <EditDiff preview={preview()!} />
      </Show>
      <Show
        when={
          props.call.name !== 'delegate' &&
          !planView(props.call) &&
          !(preview() && status() !== 'failed') &&
          detail()?.kind === 'shell'
            ? detail()
            : null
        }
      >
        <ShellCard
          summary={summary()}
          command={shellDetail()?.command ?? ''}
          output={shellDetail()?.output ?? ''}
          originalId={props.call.original_id}
          status={<StatusMark status={status()} />}
        />
      </Show>
      <Show
        when={
          detail()?.kind === 'retrieve' && props.call.name !== 'delegate' && !planView(props.call)
        }
      >
        <RetrieveCard
          summary={summary()}
          view={(detail() as { kind: 'retrieve'; view: RetrieveView }).view}
          status={<StatusMark status={status()} />}
        />
      </Show>
      <Show
        when={
          detail()?.kind === 'mcp' && !decision() && props.call.name !== 'delegate'
            ? detail()
            : null
        }
      >
        <McpCard
          summary={summary()}
          args={mcpDetail()?.args ?? ''}
          output={mcpDetail()?.output ?? ''}
          originalId={props.call.original_id}
          status={<StatusMark status={status()} />}
        />
      </Show>
      <Show
        when={
          props.call.name !== 'delegate' &&
          !planView(props.call) &&
          !(preview() && status() !== 'failed') &&
          detail()?.kind !== 'shell' &&
          detail()?.kind !== 'retrieve' &&
          detail()?.kind !== 'mcp'
        }
      >
        <div class={open() && detail() ? styles.shell : styles.call}>
          <button
            type="button"
            class={`${open() && detail() ? styles.editRow : styles.row} ${status() === 'failed' ? styles.rowFailed : ''}`}
            aria-expanded={expandable() ? open() : undefined}
            aria-invalid={status() === 'failed' ? true : undefined}
            disabled={!expandable()}
            onClick={() => {
              if (expandable()) {
                setOpen((current) => !current)
              }
            }}
          >
            <ToolIcon name={props.call.name} />
            <span class={styles.verb}>{summary().verb}</span>
            <SummaryLabel summary={summary()} />
            <StatusMark status={status()} />
          </button>
          <Show when={open() && detail()}>
            <Detail detail={detail()!} panel />
          </Show>
        </div>
      </Show>
    </Show>
  )
}

function ApprovalBody(props: {
  call: ChatToolCall
  summary: ToolSummary
  detail: ReturnType<typeof toolDetail>
  busy: boolean
  onDecide: (decision: 'approve' | 'reject') => void
}) {
  const suggestion = () => editSuggestion(props.call)
  const actions = <ApprovalActions busy={props.busy} onDecide={props.onDecide} />
  return (
    <Show when={!suggestion()} fallback={<EditDiff preview={suggestion()!} actions={actions} />}>
      <Show
        when={props.detail?.kind !== 'shell'}
        fallback={
          <ShellCard
            summary={props.summary}
            command={props.detail?.kind === 'shell' ? props.detail.command : ''}
            output={props.detail?.kind === 'shell' ? props.detail.output : ''}
            originalId={props.call.original_id}
            actions={actions}
            defaultOpen
          />
        }
      >
        <Show
          when={props.detail?.kind !== 'mcp'}
          fallback={
            <McpCard
              summary={props.summary}
              args={props.detail?.kind === 'mcp' ? props.detail.args : ''}
              output={props.detail?.kind === 'mcp' ? props.detail.output : ''}
              actions={actions}
              defaultOpen
            />
          }
        >
          <Show
            when={props.call.name === 'web_search' || props.call.name === 'web_fetch'}
            fallback={
              <div class={styles.approval}>
                <p class={styles.approvalText}>
                  <span class={styles.verb}>{props.summary.verb}</span>
                  <SummaryLabel summary={props.summary} />
                </p>
                {actions}
              </div>
            }
          >
            <div class={styles.webApproval}>
              <div class={styles.webApprovalBody}>
                <p class={styles.approvalText}>
                  <ToolIcon name={props.call.name} />
                  <span class={styles.verb}>{props.summary.verb}</span>
                  <span class={styles.target}>
                    {props.call.name === 'web_search' ? 'the web' : fetchHost(props.summary.target)}
                  </span>
                </p>
              </div>
              {actions}
            </div>
          </Show>
        </Show>
      </Show>
    </Show>
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

function PlanCard(props: {
  plan: PlanView
  buildDisabled: boolean
  onBuild: () => void
  onView: () => void
}) {
  return (
    <div class={styles.planCard}>
      <p class={styles.planStatus}>{props.plan.created ? 'Created Plan' : 'Updated Plan'}</p>
      <p class={styles.planTitle}>{props.plan.title}</p>
      <Show when={props.plan.summary}>
        <p class={styles.planSummary}>{props.plan.summary}</p>
      </Show>
      <div class={styles.planActions}>
        <button type="button" class={styles.viewPlan} onClick={() => props.onView()}>
          View Plan
        </button>
        <button
          type="button"
          class={styles.planBuild}
          disabled={props.buildDisabled}
          onClick={() => props.onBuild()}
        >
          Build
        </button>
      </div>
    </div>
  )
}

function DelegateCard(props: { call: ChatToolCall; status: 'running' | 'failed' | null }) {
  const view = () => subagentView(props.call)
  const summary = () => toolSummary(props.call)
  const mode = () => view()?.mode ?? 'explore'
  const [open, setOpen] = createSignal(false)
  const [answerOpen, setAnswerOpen] = createSignal(false)
  return (
    <div class={styles.call}>
      <button
        type="button"
        class={`${styles.row} ${props.status === 'failed' ? styles.rowFailed : ''}`}
        aria-expanded={open()}
        onClick={() => setOpen((current) => !current)}
      >
        <Show
          when={mode() === 'explore'}
          fallback={
            <>
              <span class={styles.verb}>General</span>
              <StatusMark status={props.status} />
            </>
          }
        >
          <span class={styles.exploreSummary}>
            {view() ? exploreSummary(view()!) : 'Exploring'}
          </span>
        </Show>
      </button>
      <Show when={open()}>
        <div class={styles.delegate}>
          <Show when={view()?.description || summary().target}>
            <p class={styles.delegateTitle}>{view()?.description || summary().target}</p>
          </Show>
          <Show when={view() && mode() !== 'explore'}>
            <p class={styles.meta}>{subagentCount(view()!)}</p>
          </Show>
          <Show when={(view()?.steps ?? []).length > 0}>
            <ul class={styles.steps}>
              <For each={view()?.steps ?? []}>{(step) => <StepRow step={step} />}</For>
            </ul>
          </Show>
          <Show when={view()?.answer}>
            <button
              type="button"
              class={styles.answerToggle}
              aria-expanded={answerOpen()}
              onClick={() => setAnswerOpen((current) => !current)}
            >
              Answer
            </button>
          </Show>
          <Show when={answerOpen() && view()?.answer}>
            <pre class={styles.detail}>{view()?.answer}</pre>
          </Show>
        </div>
      </Show>
    </div>
  )
}

function StepRow(props: { step: SubagentStepView }) {
  const summary = () => subagentStepSummary(props.step)
  const failed = () => props.step.status === 'denied' || props.step.status === 'failed'
  return (
    <li class={`${styles.step} ${failed() ? styles.rowFailed : ''}`}>
      <ToolIcon name={props.step.name} />
      <span class={styles.verb}>{summary().verb}</span>
      <SummaryLabel summary={summary()} />
      <Show when={props.step.status === 'running'}>
        <span class={styles.spinner} aria-label="Running" />
      </Show>
    </li>
  )
}

function SummaryLabel(props: { summary: ToolSummary }) {
  return (
    <Show when={props.summary.target || props.summary.range}>
      <Show when={props.summary.target}>
        <span class={styles.target}>{props.summary.target}</span>
      </Show>
      <Show when={props.summary.range}>
        <span class={styles.range}>{props.summary.range}</span>
      </Show>
    </Show>
  )
}

function StatusMark(props: { status: 'running' | 'failed' | null }) {
  return (
    <Show when={props.status === 'running'}>
      <span class={styles.spinner} aria-label="Running" />
    </Show>
  )
}

function fetchHost(url: string): string {
  try {
    return new URL(url).host || 'a page'
  } catch {
    return 'a page'
  }
}

function ToolIcon(props: { name: string }) {
  const icon = { class: styles.icon, size: 14, 'aria-hidden': true as const }
  if (props.name === 'retrieve') {
    return <BarsBottomLeft {...icon} />
  }
  if (props.name === 'shell') {
    return <CommandLine {...icon} />
  }
  if (props.name === 'grep' || props.name === 'find' || props.name === 'web_search') {
    return <MagnifyingGlass {...icon} />
  }
  return <Document {...icon} />
}

function ApprovalActions(props: {
  busy: boolean
  onDecide: (decision: 'approve' | 'reject') => void
}) {
  return (
    <div class={styles.actions}>
      <button
        type="button"
        class={styles.reject}
        disabled={props.busy}
        onClick={() => props.onDecide('reject')}
      >
        Reject
      </button>
      <button
        type="button"
        class={styles.approve}
        disabled={props.busy}
        onClick={() => props.onDecide('approve')}
      >
        Approve
      </button>
    </div>
  )
}

function EditDiff(props: { preview: EditPreview; actions?: JSX.Element }) {
  const [open, setOpen] = createSignal(false)
  const canExpand = () =>
    props.preview.lines.length > EDIT_VISIBLE_LINES || props.preview.hidden > 0
  const visible = () => {
    if (open()) {
      return props.preview.lines
    }
    const firstChange = props.preview.lines.findIndex((line) => line.kind !== 'context')
    const from = firstChange < 0 ? 0 : firstChange
    return props.preview.lines.slice(from, from + EDIT_VISIBLE_LINES)
  }
  return (
    <div class={styles.edit}>
      <div class={styles.editHead}>
        <button
          type="button"
          class={styles.editRow}
          aria-expanded={canExpand() ? open() : undefined}
          disabled={!canExpand()}
          onClick={() => {
            if (canExpand()) {
              setOpen((current) => !current)
            }
          }}
        >
          <span class={styles.editPath} title={props.preview.path}>
            {props.preview.path}
          </span>
          <span class={styles.editCounts}>
            <span class={styles.add}>+{props.preview.additions}</span>
            <span class={styles.del}>-{props.preview.deletions}</span>
          </span>
        </button>
        {props.actions}
      </div>
      <DiffView lines={visible()} hidden={open() ? props.preview.hidden : 0} />
    </div>
  )
}

function DiffView(props: { lines: DiffLine[]; hidden: number }) {
  return (
    <div class={styles.diff}>
      <For each={props.lines}>
        {(line) => (
          <div class={styles.diffLine} data-kind={line.kind}>
            <span class={styles.lineNo}>{line.lineNo}</span>
            <span class={styles.mark}>
              {line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' '}
            </span>
            <span class={styles.diffText}>{line.text}</span>
          </div>
        )}
      </For>
      <Show when={props.hidden > 0}>
        <p class={styles.more}>{props.hidden} more lines</p>
      </Show>
    </div>
  )
}

function useCappedOutput(
  sessionId: () => string | null,
  originalId: () => string | null | undefined,
  fallback: () => string
): () => string {
  const [loaded, setLoaded] = createSignal<{ id: string; text: string } | null>(null)
  createEffect(() => {
    const session = sessionId()
    const id = originalId()
    if (!session || !id) {
      return
    }
    let cancelled = false
    void getToolOriginal(session, id)
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
      .catch(() => {})
    onCleanup(() => {
      cancelled = true
    })
  })
  return () => {
    const current = loaded()
    const id = originalId()
    if (current && id && current.id === id) {
      return current.text
    }
    return fallback()
  }
}

function ShellCard(props: {
  summary: { verb: string; target: string }
  command: string
  output: string
  originalId?: string | null
  actions?: JSX.Element
  status?: JSX.Element
  defaultOpen?: boolean
}) {
  const shown = useCappedOutput(
    () => chat.activeSessionId,
    () => props.originalId,
    () => props.output
  )
  const [open, setOpen] = createSignal(props.defaultOpen ?? false)
  return (
    <Show
      when={open()}
      fallback={
        <div class={styles.editHead}>
          <button
            type="button"
            class={styles.row}
            aria-expanded={false}
            onClick={() => setOpen(true)}
          >
            <ToolIcon name="shell" />
            <span class={styles.verb}>{props.summary.verb}</span>
            <SummaryLabel summary={props.summary} />
            {props.status}
          </button>
          {props.actions}
        </div>
      }
    >
      <div class={styles.shell}>
        <div class={styles.editHead}>
          <button
            type="button"
            class={styles.editRow}
            aria-expanded={true}
            onClick={() => setOpen(false)}
          >
            <ToolIcon name="shell" />
            <span class={styles.verb}>{props.summary.verb}</span>
            <SummaryLabel summary={props.summary} />
            {props.status}
          </button>
          {props.actions}
        </div>
        <ShellBody command={props.command} output={shown()} compact={!props.actions} />
      </div>
    </Show>
  )
}

function McpCard(props: {
  summary: { verb: string; target: string }
  args: string
  output: string
  originalId?: string | null
  actions?: JSX.Element
  status?: JSX.Element
  defaultOpen?: boolean
}) {
  const shown = useCappedOutput(
    () => chat.activeSessionId,
    () => props.originalId,
    () => props.output
  )
  const [open, setOpen] = createSignal(props.defaultOpen ?? false)
  return (
    <div class={open() ? styles.shell : styles.call}>
      <div class={styles.editHead}>
        <button
          type="button"
          class={open() ? styles.editRow : styles.row}
          aria-expanded={open()}
          onClick={() => setOpen((current) => !current)}
        >
          <span class={styles.verb}>{props.summary.verb}</span>
          <SummaryLabel summary={props.summary} />
          {props.status}
        </button>
        {props.actions}
      </div>
      <Show when={open()}>
        <pre class={styles.shellBody}>
          {props.args}
          {shown() ? `\n${shown()}` : ''}
        </pre>
      </Show>
    </div>
  )
}

function RetrieveCard(props: { summary: ToolSummary; view: RetrieveView; status?: JSX.Element }) {
  const [open, setOpen] = createSignal(false)
  const facts = () => {
    const view = props.view
    const page =
      view.totalLines > 0
        ? `lines ${view.startLine}–${view.endLine || view.startLine} of ${view.totalLines}`
        : null
    return [
      view.stream === 'both' ? 'stdout and stderr' : view.stream,
      view.exitCode != null ? `exit ${view.exitCode}` : null,
      view.truncated ? 'truncated' : null,
      view.raw ? 'raw' : null,
      page,
      view.nextOffset != null ? `more from L${view.nextOffset}` : null
    ].filter((fact): fact is string => fact != null && fact.length > 0)
  }
  return (
    <div class={open() ? styles.shell : styles.call}>
      <button
        type="button"
        class={open() ? styles.editRow : styles.row}
        aria-expanded={open()}
        onClick={() => setOpen((current) => !current)}
      >
        <ToolIcon name="retrieve" />
        <span class={styles.verb}>{props.summary.verb}</span>
        <SummaryLabel summary={props.summary} />
        {props.status}
      </button>
      <Show when={open()}>
        <div class={styles.retrieve}>
          <p class={styles.retrieveMeta}>{facts().join(' · ')}</p>
          <Show when={props.view.stdout}>
            <LogLines text={props.view.stdout} startLine={props.view.startLine} />
          </Show>
          <Show when={props.view.stderr}>
            <p class={styles.retrieveStream}>stderr</p>
            <LogLines
              text={props.view.stderr}
              startLine={props.view.stream === 'stderr' ? props.view.startLine : 1}
            />
          </Show>
          <Show when={!props.view.stdout && !props.view.stderr}>
            <p class={styles.retrieveMeta}>Empty page</p>
          </Show>
        </div>
      </Show>
    </div>
  )
}

function LogLines(props: { text: string; startLine: number }) {
  const lines = () => props.text.split('\n')
  return (
    <div class={styles.retrieveLog}>
      <For each={lines()}>
        {(line, index) => (
          <span class={line.startsWith('<<<ROBI_') ? styles.markerLine : styles.codeLine}>
            <span class={styles.lineNo}>{props.startLine + index()}</span>
            <span>{line}</span>
          </span>
        )}
      </For>
    </div>
  )
}

function ShellBody(props: { command: string; output: string; compact?: boolean }) {
  return (
    <pre
      class={props.compact ? `${styles.shellBody} ${styles.shellBodyCompact}` : styles.shellBody}
    >
      <span class={styles.shellCommand}>
        <span class={styles.prompt}>$</span> {props.command}
      </span>
      {props.output}
    </pre>
  )
}

function Detail(props: { detail: NonNullable<ReturnType<typeof toolDetail>>; panel?: boolean }) {
  const body = () => (props.panel ? styles.shellBody : styles.detail)
  return (
    <Show
      when={props.detail.kind !== 'error'}
      fallback={
        <pre class={props.panel ? `${styles.shellBody} ${styles.errorText}` : styles.error}>
          {props.detail.kind === 'error' ? props.detail.text : ''}
        </pre>
      }
    >
      <Show
        when={props.detail.kind === 'code'}
        fallback={
          <pre class={body()}>
            <For each={props.detail.kind === 'lines' ? props.detail.lines : []}>
              {(line) => <span class={styles.plainLine}>{line}</span>}
            </For>
          </pre>
        }
      >
        <pre class={body()}>
          <For each={props.detail.kind === 'code' ? props.detail.lines : []}>
            {(line, index) => (
              <span class={styles.codeLine}>
                <span class={styles.lineNo}>
                  {(props.detail.kind === 'code' ? props.detail.startLine : 1) + index()}
                </span>
                <span>{line}</span>
              </span>
            )}
          </For>
        </pre>
      </Show>
    </Show>
  )
}
