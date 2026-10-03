import {
  Fragment,
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject
} from 'react'

import type { ChatMessage } from '../../api/messages'
import type { AgentMode } from '../../api/sessions'
import type { AgentPhase } from '../../state/chatStore'
import { AssistantMarkdown } from './AssistantMarkdown'
import { CopyMarkdownButton } from './CopyMarkdownButton'
import styles from './Transcript.module.css'
import { FinishedTodos, RemainingTodos } from './TodoList'
import { ToolCallCard } from './ToolCallCard'
import { finishedTodos, remainingTodos } from './todoProgress'
import type { PlanView } from './toolCallView'
import { distanceFromBottom } from './transcriptFollow'
import { formatElapsed, pendingSeconds, workedLabel } from './turnDuration'

type TranscriptProps = {
  messages: ChatMessage[]
  echo: string | null
  phase: AgentPhase
  /** Open tasks stay off until a build starts in agent mode. */
  mode: AgentMode
  deciding: boolean
  buildDisabled?: boolean
  onDecide: (callId: string, decision: 'approve' | 'reject') => void
  onBuild?: (path: string) => void
  onViewPlan?: (plan: PlanView) => void
}

const FOLLOW_THRESHOLD = 48

function ThinkingDots() {
  const [count, setCount] = useState(1)
  const [reduced, setReduced] = useState(false)

  useEffect(() => {
    const media = window.matchMedia('(prefers-reduced-motion: reduce)')
    const apply = () => setReduced(media.matches)
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [])

  useEffect(() => {
    if (reduced) {
      return
    }
    const id = window.setInterval(() => {
      setCount((current) => (current % 3) + 1)
    }, 400)
    return () => window.clearInterval(id)
  }, [reduced])

  return (
    <span className={styles.dots} aria-hidden>
      {reduced ? '...' : '.'.repeat(count)}
    </span>
  )
}

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

function renderAssistant(
  message: ChatMessage,
  phase: AgentPhase,
  deciding: boolean,
  buildDisabled: boolean,
  onDecide: (callId: string, decision: 'approve' | 'reject') => void,
  onBuild: ((path: string) => void) | undefined,
  onViewPlan: ((plan: PlanView) => void) | undefined
) {
  if (message.role !== 'assistant') {
    return null
  }
  const text = message.content.trim()
  if (!text && message.tool_calls.length === 0) {
    return null
  }
  return (
    <Fragment key={message.id}>
      {text ? (
        <div className={styles.prose}>
          <AssistantMarkdown text={message.content} />
        </div>
      ) : null}
      {message.tool_calls.map((call) => {
        if (call.name === 'todos' && !call.error && call.execution_status === 'succeeded') {
          return <FinishedTodos key={call.id} items={finishedTodos(call)} />
        }
        return (
          <ToolCallCard
            key={call.id}
            call={call}
            phase={phase}
            busy={deciding}
            buildDisabled={buildDisabled}
            onDecide={(decision) => onDecide(call.id, decision)}
            onBuild={onBuild}
            onViewPlan={onViewPlan}
          />
        )
      })}
    </Fragment>
  )
}

function TurnFooter({ worked, text }: { worked: string | null; text: string | undefined }) {
  if (!worked && !text) {
    return null
  }
  return (
    <div className={styles.footer}>
      <span className={styles.worked}>{worked}</span>
      {text ? <CopyMarkdownButton text={text} /> : null}
    </div>
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

function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!active) {
      return
    }
    const refresh = () => setNow(Date.now())
    const frame = window.requestAnimationFrame(refresh)
    const id = window.setInterval(refresh, 1000)
    return () => {
      window.cancelAnimationFrame(frame)
      window.clearInterval(id)
    }
  }, [active])
  return now
}

function useLocalStart(active: boolean): number | null {
  const [start, setStart] = useState<number | null>(null)
  useEffect(() => {
    if (!active) {
      return
    }
    const frame = window.requestAnimationFrame(() => setStart(Date.now()))
    return () => window.cancelAnimationFrame(frame)
  }, [active])
  return active ? start : null
}

type TurnViewProps = {
  group: ChatMessage[]
  last: boolean
  phase: AgentPhase
  deciding: boolean
  buildDisabled: boolean
  onDecide: (callId: string, decision: 'approve' | 'reject') => void
  onBuild?: (path: string) => void
  onViewPlan?: (plan: PlanView) => void
}

function sameGroup(left: ChatMessage[], right: ChatMessage[]): boolean {
  if (left.length !== right.length) {
    return false
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false
    }
  }
  return true
}

const TurnView = memo(function TurnView({
  group,
  last,
  phase,
  deciding,
  buildDisabled,
  onDecide,
  onBuild,
  onViewPlan
}: TurnViewProps) {
  const start = group.find((message) => message.role === 'user')
  const end = group[group.length - 1]
  const open = turnStillOpen(group, last, phase)
  const worked = !open && start && end && end.id !== start.id ? workedLabel(start.id, end.id) : null
  const assistant = group.filter((message) => message.role === 'assistant')
  return (
    <Fragment>
      {start ? (
        <li key={start.id} className={styles.user}>
          {start.content}
          {start.skills?.map((skill) => (
            <details key={skill.id} className={styles.skill}>
              <summary>Using {skill.id}</summary>
              <p>{skill.description}</p>
              <p className={styles.skillPath}>{skill.directory}</p>
              <pre>{skill.body}</pre>
            </details>
          ))}
        </li>
      ) : null}
      {assistant.length > 0 ? (
        <li key={`${group[0].id}-assistant`} className={styles.turn}>
          {assistant.map((message) =>
            renderAssistant(message, phase, deciding, buildDisabled, onDecide, onBuild, onViewPlan)
          )}
          <TurnFooter
            worked={worked}
            text={
              open
                ? undefined
                : assistant
                    .slice()
                    .reverse()
                    .find((item) => item.content.trim())?.content
            }
          />
        </li>
      ) : null}
    </Fragment>
  )
}, turnViewPropsEqual)

function turnViewPropsEqual(prev: TurnViewProps, next: TurnViewProps): boolean {
  if (!sameGroup(prev.group, next.group) || prev.last !== next.last) {
    return false
  }
  if (!next.last) {
    return true
  }
  return (
    prev.phase === next.phase &&
    prev.deciding === next.deciding &&
    prev.buildDisabled === next.buildDisabled &&
    prev.onDecide === next.onDecide &&
    prev.onBuild === next.onBuild &&
    prev.onViewPlan === next.onViewPlan
  )
}

function ActivityLine({ phase, anchorId }: { phase: AgentPhase; anchorId: string | undefined }) {
  const label = activityLabel(phase)
  const active = label != null
  const now = useNow(active)
  const localStart = useLocalStart(active)
  const fromMessage = label ? pendingSeconds(anchorId, now) : null
  const fromLocal = localStart == null ? null : Math.floor((now - localStart) / 1000)
  const pending = fromMessage ?? fromLocal
  const pendingText = pending != null && pending >= 1 ? `for ${formatElapsed(pending)}` : ''
  if (!label) {
    return null
  }
  return (
    <li className={styles.activity}>
      <span>
        <span aria-live="polite">{label}</span>
        {pendingText ? (
          <>
            {' '}
            <span aria-hidden>{pendingText}</span>
          </>
        ) : null}
      </span>
      <ThinkingDots />
    </li>
  )
}

function nestedScrollsUp(target: EventTarget | null, scroller: HTMLElement): boolean {
  let current = target instanceof Element ? target : null
  while (current && current !== scroller) {
    if (current.scrollTop > 0 && current.scrollHeight > current.clientHeight + 1) {
      return true
    }
    current = current.parentElement
  }
  return false
}

function useFollowTail(
  scrollerRef: RefObject<HTMLDivElement | null>,
  headId: string,
  messages: ChatMessage[],
  echo: string | null,
  phase: AgentPhase
) {
  const pinned = useRef(true)
  const adjusting = useRef(0)
  const lastTop = useRef(0)
  const headRef = useRef(headId)

  useEffect(() => {
    const scroller = scrollerRef.current
    if (!scroller) {
      return
    }
    const onScroll = () => {
      const top = scroller.scrollTop
      const previous = lastTop.current
      lastTop.current = top
      if (adjusting.current > 0) {
        return
      }
      const nearBottom = distanceFromBottom(scroller) <= FOLLOW_THRESHOLD
      if (top < previous - 1 && !nearBottom) {
        pinned.current = false
        return
      }
      pinned.current = nearBottom
    }
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY >= 0 || nestedScrollsUp(event.target, scroller)) {
        return
      }
      pinned.current = false
    }
    scroller.addEventListener('scroll', onScroll, { passive: true })
    scroller.addEventListener('wheel', onWheel, { passive: true })
    return () => {
      scroller.removeEventListener('scroll', onScroll)
      scroller.removeEventListener('wheel', onWheel)
    }
  }, [scrollerRef])

  useLayoutEffect(() => {
    const scroller = scrollerRef.current
    if (!scroller) {
      return
    }
    if (headRef.current !== headId) {
      headRef.current = headId
      pinned.current = true
    }
    const follow = () => {
      if (!pinned.current) {
        return
      }
      const next = scroller.scrollHeight - scroller.clientHeight
      if (next <= scroller.scrollTop + 1) {
        return
      }
      adjusting.current += 1
      scroller.scrollTop = next
      lastTop.current = scroller.scrollTop
      window.requestAnimationFrame(() => {
        adjusting.current = Math.max(0, adjusting.current - 1)
      })
    }
    follow()
    const content = scroller.firstElementChild
    if (content == null || typeof ResizeObserver === 'undefined') {
      return
    }
    const observer = new ResizeObserver(() => follow())
    observer.observe(content)
    return () => observer.disconnect()
  }, [scrollerRef, headId, messages, echo, phase])
}

export function Transcript({
  messages,
  echo,
  phase,
  mode,
  deciding,
  buildDisabled = false,
  onDecide,
  onBuild,
  onViewPlan
}: TranscriptProps) {
  const scrollerRef = useRef<HTMLDivElement>(null)
  const onDecideRef = useRef(onDecide)
  const onBuildRef = useRef(onBuild)
  const onViewPlanRef = useRef(onViewPlan)
  useEffect(() => {
    onDecideRef.current = onDecide
    onBuildRef.current = onBuild
    onViewPlanRef.current = onViewPlan
  }, [onDecide, onBuild, onViewPlan])
  const decide = useCallback((callId: string, decision: 'approve' | 'reject') => {
    onDecideRef.current(callId, decision)
  }, [])
  const build = useCallback((path: string) => {
    onBuildRef.current?.(path)
  }, [])
  const viewPlan = useCallback((plan: PlanView) => {
    onViewPlanRef.current?.(plan)
  }, [])
  const turns = groupTurns(messages)
  const anchorId = echo
    ? undefined
    : [...messages].reverse().find((message) => message.role === 'user')?.id
  useFollowTail(scrollerRef, messages[0]?.id ?? '', messages, echo, phase)

  return (
    <div className={styles.transcript} ref={scrollerRef}>
      <ul className={styles.list}>
        {turns.map((group, index) => (
          <TurnView
            key={group[0].id}
            group={group}
            last={index === turns.length - 1}
            phase={phase}
            deciding={deciding}
            buildDisabled={buildDisabled}
            onDecide={decide}
            onBuild={onBuild ? build : undefined}
            onViewPlan={onViewPlan ? viewPlan : undefined}
          />
        ))}
        {echo ? (
          <li key="pending-echo" className={styles.user}>
            {echo}
          </li>
        ) : null}
        <RemainingTodos items={mode === 'agent' ? remainingTodos(messages) : []} />
        <ActivityLine phase={phase} anchorId={anchorId} />
      </ul>
    </div>
  )
}
