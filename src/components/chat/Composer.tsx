import { useState } from 'react'

import type { AgentPhase } from '../../state/chatStore'
import styles from './Composer.module.css'

type ComposerProps = {
  disabled: boolean
  phase: AgentPhase
  onSubmit: (text: string) => Promise<boolean>
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

export function Composer({ disabled, phase, onSubmit }: ComposerProps) {
  const [draft, setDraft] = useState('')
  const label = statusLabel(phase)
  const canSend = !disabled && draft.trim().length > 0

  async function submit() {
    if (!canSend) {
      return
    }
    const sent = await onSubmit(draft)
    if (sent) {
      setDraft('')
    }
  }

  return (
    <div className={styles.composer}>
      <div className={styles.column}>
        <div className={styles.field}>
          <textarea
            className={styles.input}
            rows={1}
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

        <div className={styles.toolbar}>
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
        </div>
      </div>
    </div>
  )
}
