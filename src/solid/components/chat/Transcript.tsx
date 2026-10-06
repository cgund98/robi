/** @jsxImportSource solid-js */
import { createEffect, createSignal, For, onCleanup, onMount, Show } from 'solid-js'

import type { ChatMessage } from '../../../api/messages'
import { imageUrl } from '../../../api/messages'
import type { AgentMode } from '../../../api/sessions'
import type { AgentPhase } from '../../../state/chatStore'
import type { AttachmentMeta } from '../../../components/chat/textAttachments'
import { finishedTodos, remainingTodos } from '../../../components/chat/todoProgress'
import type { PlanView } from '../../../components/chat/toolCallView'
import { distanceFromBottom } from '../../../components/chat/transcriptFollow'
import { formatElapsed, pendingSeconds, workedLabel } from '../../../components/chat/turnDuration'
import styles from '../../../components/chat/Transcript.module.css'
import { AssistantMarkdown } from './AssistantMarkdown'
import { AttachmentChip } from './AttachmentChip'
import { CopyMarkdownButton } from './CopyMarkdownButton'
import { FinishedTodos, RemainingTodos } from './TodoList'
import { ToolCallCard } from './ToolCallCard'

const FOLLOW_THRESHOLD = 48

function groupTurns(messages: ChatMessage[]): ChatMessage[][] {
  const groups: ChatMessage[][] = []
  for (const message of messages) {
    if (message.role === 'user' || groups.length === 0) {
      groups.push([message])
    } else {
      groups[groups.length - 1].push(message)
    }
  }
  return groups
}

function turnStillOpen(group: ChatMessage[], last: boolean, phase: AgentPhase): boolean {
  if (!last) {
    return false
  }
  if (phase !== 'idle') {
    return true
  }
  return group.some((message) =>
    message.tool_calls.some(
      (call) => call.approval_status === 'pending' && call.execution_status === 'not_started'
    )
  )
}

function activityLabel(phase: AgentPhase): string | null {
  if (phase === 'thinking') {
    return 'Thinking'
  }
  if (phase === 'responding') {
    return 'Responding'
  }
  return null
}

export function Transcript(props: {
  messages: ChatMessage[]
  sessionId?: string | null
  echo: string | null
  echoFiles?: AttachmentMeta[]
  phase: AgentPhase
  mode: AgentMode
  deciding: boolean
  buildDisabled?: boolean
  onDecide: (callId: string, decision: 'approve' | 'reject') => void
  onBuild?: (path: string) => void
  onViewPlan?: (plan: PlanView) => void
}) {
  let scroller: HTMLDivElement | undefined
  let pinned = true
  let adjusting = 0
  let lastTop = 0
  let headId = ''
  const turns = () => groupTurns(props.messages)
  const anchorId = () => (props.echo ? undefined : props.messages[props.messages.length - 1]?.id)

  const follow = () => {
    const node = scroller
    if (!node) {
      return
    }
    const nextHead = props.messages[0]?.id ?? ''
    if (headId !== nextHead) {
      headId = nextHead
      pinned = true
    }
    if (!pinned) {
      return
    }
    const next = node.scrollHeight - node.clientHeight
    if (next <= node.scrollTop + 1) {
      return
    }
    adjusting += 1
    node.scrollTop = next
    lastTop = node.scrollTop
    window.requestAnimationFrame(() => {
      adjusting = Math.max(0, adjusting - 1)
    })
  }

  onMount(() => {
    const node = scroller
    if (!node) {
      return
    }
    const onScroll = () => {
      const top = node.scrollTop
      const previous = lastTop
      lastTop = top
      if (adjusting > 0) {
        return
      }
      const nearBottom = distanceFromBottom(node) <= FOLLOW_THRESHOLD
      if (top < previous - 1 && !nearBottom) {
        pinned = false
        return
      }
      pinned = nearBottom
    }
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY >= 0) {
        return
      }
      pinned = false
    }
    node.addEventListener('scroll', onScroll, { passive: true })
    node.addEventListener('wheel', onWheel, { passive: true })
    const content = node.firstElementChild
    const observer =
      content && typeof ResizeObserver !== 'undefined' ? new ResizeObserver(() => follow()) : null
    if (content && observer) {
      observer.observe(content)
    }
    onCleanup(() => {
      node.removeEventListener('scroll', onScroll)
      node.removeEventListener('wheel', onWheel)
      observer?.disconnect()
    })
  })

  createEffect(() => {
    props.messages
    props.echo
    props.phase
    follow()
  })

  return (
    <div class={styles.transcript} ref={scroller}>
      <ul class={styles.list}>
        <For each={turns()}>
          {(group, index) => (
            <TurnView
              group={group}
              sessionId={props.sessionId}
              last={index() === turns().length - 1}
              phase={props.phase}
              deciding={props.deciding}
              buildDisabled={props.buildDisabled ?? false}
              onDecide={props.onDecide}
              onBuild={props.onBuild}
              onViewPlan={props.onViewPlan}
            />
          )}
        </For>
        <Show when={props.echo}>
          <li class={styles.user}>
            <Show when={(props.echoFiles ?? []).length > 0}>
              <div class={styles.files}>
                <For each={props.echoFiles ?? []}>
                  {(file) => (
                    <AttachmentChip
                      name={file.name}
                      path={file.path}
                      startLine={file.startLine}
                      endLine={file.endLine}
                    />
                  )}
                </For>
              </div>
            </Show>
            {props.echo}
          </li>
        </Show>
        <RemainingTodos items={props.mode === 'agent' ? remainingTodos(props.messages) : []} />
        <ActivityLine phase={props.phase} anchorId={anchorId()} />
      </ul>
    </div>
  )
}

function TurnView(props: {
  group: ChatMessage[]
  sessionId?: string | null
  last: boolean
  phase: AgentPhase
  deciding: boolean
  buildDisabled: boolean
  onDecide: (callId: string, decision: 'approve' | 'reject') => void
  onBuild?: (path: string) => void
  onViewPlan?: (plan: PlanView) => void
}) {
  const start = () => props.group.find((message) => message.role === 'user')
  const end = () => props.group[props.group.length - 1]
  const open = () => turnStillOpen(props.group, props.last, props.phase)
  const assistant = () => props.group.filter((message) => message.role === 'assistant')
  const worked = () => {
    const first = start()
    const last = end()
    if (open() || !first || !last || last.id === first.id) {
      return null
    }
    return workedLabel(first.id, last.id)
  }
  const copyText = () =>
    open()
      ? undefined
      : assistant()
          .slice()
          .reverse()
          .find((item) => item.content.trim())?.content

  return (
    <Show when={!start()?.compaction} fallback={<ContextCompactedDivider />}>
      <Show when={start()}>
        {(user) => (
          <li class={styles.user}>
            <Show when={user().images && user().images!.length > 0 && props.sessionId}>
              <div class={styles.images}>
                <For each={user().images ?? []}>
                  {(image) => (
                    <img src={imageUrl(props.sessionId!, image.id)} alt="" class={styles.bubble} />
                  )}
                </For>
              </div>
            </Show>
            <Show when={user().files && user().files!.length > 0}>
              <div class={styles.files}>
                <For each={user().files ?? []}>
                  {(file) => (
                    <AttachmentChip
                      name={file.name}
                      path={file.path}
                      startLine={file.start_line}
                      endLine={file.end_line}
                    />
                  )}
                </For>
              </div>
            </Show>
            {user().content}
            <For each={user().skills ?? []}>
              {(skill) => (
                <details class={styles.skill}>
                  <summary>Using {skill.id}</summary>
                  <p>{skill.description}</p>
                  <p class={styles.skillPath}>{skill.directory}</p>
                  <pre>{skill.body}</pre>
                </details>
              )}
            </For>
          </li>
        )}
      </Show>
      <Show when={assistant().length > 0}>
        <li class={styles.turn}>
          <For each={assistant()}>
            {(message) => (
              <AssistantBlock
                message={message}
                phase={props.phase}
                deciding={props.deciding}
                buildDisabled={props.buildDisabled}
                onDecide={props.onDecide}
                onBuild={props.onBuild}
                onViewPlan={props.onViewPlan}
              />
            )}
          </For>
          <TurnFooter worked={worked()} text={copyText()} />
        </li>
      </Show>
    </Show>
  )
}

function AssistantBlock(props: {
  message: ChatMessage
  phase: AgentPhase
  deciding: boolean
  buildDisabled: boolean
  onDecide: (callId: string, decision: 'approve' | 'reject') => void
  onBuild?: (path: string) => void
  onViewPlan?: (plan: PlanView) => void
}) {
  const text = () => props.message.content.trim()
  return (
    <Show
      when={props.message.role === 'assistant' && (text() || props.message.tool_calls.length > 0)}
    >
      <Show when={text()}>
        <div class={styles.prose}>
          <AssistantMarkdown text={props.message.content} />
        </div>
      </Show>
      <For each={props.message.tool_calls}>
        {(call) => (
          <Show
            when={call.name === 'todos' && !call.error && call.execution_status === 'succeeded'}
            fallback={
              <ToolCallCard
                call={call}
                phase={props.phase}
                busy={props.deciding}
                buildDisabled={props.buildDisabled}
                onDecide={(decision) => props.onDecide(call.id, decision)}
                onBuild={props.onBuild}
                onViewPlan={props.onViewPlan}
              />
            }
          >
            <FinishedTodos items={finishedTodos(call)} />
          </Show>
        )}
      </For>
    </Show>
  )
}

function TurnFooter(props: { worked: string | null; text: string | undefined }) {
  return (
    <Show when={props.worked || props.text}>
      <div class={styles.footer}>
        <span class={styles.worked}>{props.worked}</span>
        <Show when={props.text}>{(text) => <CopyMarkdownButton text={text()} />}</Show>
      </div>
    </Show>
  )
}

function ContextCompactedDivider() {
  return (
    <li class={styles.compacted} role="separator">
      <span class={styles.compactedLabel}>Context compacted</span>
    </li>
  )
}

function ThinkingDots() {
  const [count, setCount] = createSignal(1)
  const [reduced, setReduced] = createSignal(false)
  onMount(() => {
    const media = window.matchMedia('(prefers-reduced-motion: reduce)')
    const apply = () => setReduced(media.matches)
    apply()
    media.addEventListener('change', apply)
    onCleanup(() => media.removeEventListener('change', apply))
  })
  createEffect(() => {
    if (reduced()) {
      return
    }
    const id = window.setInterval(() => setCount((current) => (current % 3) + 1), 400)
    onCleanup(() => window.clearInterval(id))
  })
  return (
    <span class={styles.dots} aria-hidden="true">
      {reduced() ? '...' : '.'.repeat(count())}
    </span>
  )
}

function ActivityLine(props: { phase: AgentPhase; anchorId: string | undefined }) {
  const label = () => activityLabel(props.phase)
  const [now, setNow] = createSignal(Date.now())
  const [localStart, setLocalStart] = createSignal<number | null>(null)
  createEffect(() => {
    if (!label()) {
      return
    }
    const refresh = () => setNow(Date.now())
    const frame = window.requestAnimationFrame(refresh)
    const id = window.setInterval(refresh, 1000)
    const startFrame = window.requestAnimationFrame(() => setLocalStart(Date.now()))
    onCleanup(() => {
      window.cancelAnimationFrame(frame)
      window.cancelAnimationFrame(startFrame)
      window.clearInterval(id)
    })
  })
  const pendingText = () => {
    if (!label()) {
      return ''
    }
    const fromMessage = pendingSeconds(props.anchorId, now())
    const start = localStart()
    const fromLocal = start == null ? null : Math.floor((now() - start) / 1000)
    const pending = fromMessage ?? fromLocal
    return pending != null && pending >= 1 ? `for ${formatElapsed(pending)}` : ''
  }
  return (
    <Show when={label()}>
      {(text) => (
        <li class={styles.activity}>
          <span>
            <span aria-live="polite">{text()}</span>
            <Show when={pendingText()}>
              {' '}
              <span aria-hidden="true">{pendingText()}</span>
            </Show>
          </span>
          <ThinkingDots />
        </li>
      )}
    </Show>
  )
}
