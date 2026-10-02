import { Fragment, useEffect, useRef, useState } from 'react'

import type { ChatMessage } from '../../api/messages'
import type { AgentPhase } from '../../state/chatStore'
import { AssistantMarkdown } from './AssistantMarkdown'
import { CopyMarkdownButton } from './CopyMarkdownButton'
import styles from './Transcript.module.css'
import { ToolCallCard } from './ToolCallCard'
import { formatElapsed, pendingSeconds, workedLabel } from './turnDuration'

type TranscriptProps = {
  messages: ChatMessage[]
  echo: string | null
  phase: AgentPhase
  deciding: boolean
  onDecide: (callId: string, decision: 'approve' | 'reject') => void
}

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
  onDecide: (callId: string, decision: 'approve' | 'reject') => void
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
      {message.tool_calls.map((call) => (
        <ToolCallCard
          key={call.id}
          call={call}
          phase={phase}
          busy={deciding}
          onDecide={(decision) => onDecide(call.id, decision)}
        />
      ))}
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
    setNow(Date.now())
    const id = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [active])
  return now
}

export function Transcript({ messages, echo, phase, deciding, onDecide }: TranscriptProps) {
  const scrollerRef = useRef<HTMLDivElement>(null)
  const label = activityLabel(phase)
  const now = useNow(label != null)
  const localStart = useRef<number | null>(null)
  if (label && localStart.current == null) {
    localStart.current = Date.now()
  }
  if (!label) {
    localStart.current = null
  }
  const turns = groupTurns(messages)
  const pendingStart = echo
    ? undefined
    : [...messages].reverse().find((message) => message.role === 'user')?.id
  const fromMessage = label ? pendingSeconds(pendingStart, now) : null
  const fromLocal =
    localStart.current == null ? null : Math.floor((now - localStart.current) / 1000)
  const pending = fromMessage ?? fromLocal
  const pendingText = pending != null && pending >= 1 ? `for ${formatElapsed(pending)}` : ''

  useEffect(() => {
    const scroller = scrollerRef.current
    if (!scroller) {
      return
    }
    scroller.scrollTop = scroller.scrollHeight
  }, [messages, echo, phase])

  return (
    <div className={styles.transcript} ref={scrollerRef}>
      <ul className={styles.list}>
        {turns.map((group, index) => {
          const start = group.find((message) => message.role === 'user')
          const end = group[group.length - 1]
          const open = turnStillOpen(group, index === turns.length - 1, phase)
          const worked =
            !open && start && end && end.id !== start.id ? workedLabel(start.id, end.id) : null
          const assistant = group.filter((message) => message.role === 'assistant')
          return (
            <Fragment key={group[0].id}>
              {start ? (
                <li key={start.id} className={styles.user}>
                  {start.content}
                </li>
              ) : null}
              {assistant.length > 0 ? (
                <li key={`${group[0].id}-assistant`} className={styles.turn}>
                  {assistant.map((message) => renderAssistant(message, phase, deciding, onDecide))}
                  <TurnFooter
                    worked={worked}
                    text={
                      assistant
                        .slice()
                        .reverse()
                        .find((item) => item.content.trim())?.content
                    }
                  />
                </li>
              ) : null}
            </Fragment>
          )
        })}
        {echo ? (
          <li key="pending-echo" className={styles.user}>
            {echo}
          </li>
        ) : null}
        {label ? (
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
        ) : null}
      </ul>
    </div>
  )
}
