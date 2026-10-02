import { useEffect, useRef, type ReactNode } from 'react'

import type { ChatMessage } from '../../api/messages'
import type { AgentPhase } from '../../state/chatStore'
import styles from './Transcript.module.css'

type TranscriptProps = {
  messages: ChatMessage[]
  echo: string | null
  phase: AgentPhase
}

function renderInlineCode(text: string): ReactNode[] {
  const parts = text.split(/(`[^`]+`)/g)
  return parts.map((part, index) => {
    if (part.startsWith('`') && part.endsWith('`') && part.length > 2) {
      return <code key={index}>{part.slice(1, -1)}</code>
    }
    return <span key={index}>{part}</span>
  })
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

export function Transcript({ messages, echo, phase }: TranscriptProps) {
  const scrollerRef = useRef<HTMLDivElement>(null)
  const label = activityLabel(phase)

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
        {messages.map((message) => {
          if (message.role === 'user') {
            return (
              <li key={message.id} className={styles.user}>
                {message.content}
              </li>
            )
          }
          if (message.role === 'assistant') {
            return (
              <li key={message.id} className={styles.assistant}>
                {renderInlineCode(message.content)}
              </li>
            )
          }
          if (!message.content.trim()) {
            return null
          }
          return (
            <li key={message.id} className={styles.activity}>
              {message.content}
            </li>
          )
        })}
        {echo ? (
          <li key="pending-echo" className={styles.user}>
            {echo}
          </li>
        ) : null}
        {label ? (
          <li className={styles.activity} aria-live="polite">
            <span className={`${styles.activityIcon} ${styles.activityIconLive}`} aria-hidden>
              ●
            </span>
            {label}
          </li>
        ) : null}
      </ul>
    </div>
  )
}
