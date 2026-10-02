import { useState } from 'react'

import type { AgentPhase } from '../../state/chatStore'
import styles from './Composer.module.css'

type ComposerProps = {
  disabled: boolean
  phase: AgentPhase
  onSubmit: (text: string) => Promise<boolean>
  /** Centered card on an empty chat. Dock keeps the field at the bottom of a thread. */
  placement?: 'dock' | 'welcome'
}

function statusLabel(phase: AgentPhase): string {
  if (phase === 'thinking') {
    return 'Thinking'
  }
  if (phase === 'responding') {
    return 'Responding'
  }
  return 'Idle'
}

export function Composer({ disabled, phase, onSubmit, placement = 'dock' }: ComposerProps) {
  const [draft, setDraft] = useState('')
  const label = statusLabel(phase)
  const canSend = !disabled && draft.trim().length > 0
  const welcome = placement === 'welcome'

  async function submit() {
    if (!canSend) {
      return
    }
    const sent = await onSubmit(draft)
    if (sent) {
      setDraft('')
    }
  }

  const controls = (
    <div className={styles.cluster}>
      <button type="button" className={styles.control}>
        Model ▾
      </button>
      <button type="button" className={styles.control}>
        Extra high ▾
      </button>
      <span
        className={`${styles.status} ${phase === 'idle' ? '' : styles.statusBusy}`}
        title={label}
      />
    </div>
  )

  return (
    <div className={welcome ? styles.welcome : styles.composer}>
      <div className={styles.column}>
        <div className={welcome ? styles.card : styles.field}>
          <textarea
            className={welcome ? styles.cardInput : styles.input}
            rows={welcome ? 2 : 1}
            placeholder="Describe a task or ask a question"
            value={draft}
            disabled={disabled}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault()
                void submit()
              }
            }}
            aria-label="Message"
          />
          {welcome ? (
            <div className={styles.cardBar}>
              {controls}
              <button
                type="button"
                className={styles.send}
                disabled={!canSend}
                aria-label="Send"
                onClick={() => void submit()}
              >
                ⏎
              </button>
            </div>
          ) : (
            <button
              type="button"
              className={styles.send}
              disabled={!canSend}
              aria-label="Send"
              onClick={() => void submit()}
            >
              ⏎
            </button>
          )}
        </div>

        {welcome ? null : <div className={styles.toolbar}>{controls}</div>}
      </div>
    </div>
  )
}
