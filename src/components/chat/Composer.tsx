import { useState } from 'react'

import type { CatalogModel } from '../../api/models'
import type { AgentPhase } from '../../state/chatStore'
import { ChoiceMenu } from './ChoiceMenu'
import styles from './Composer.module.css'

const EFFORTS = [
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' }
]

type ComposerProps = {
  disabled: boolean
  phase: AgentPhase
  onSubmit: (text: string) => Promise<boolean>
  /** Centered card on an empty chat. Dock keeps the field at the bottom of a thread. */
  placement?: 'dock' | 'welcome'
  models: CatalogModel[]
  /** Session override. Null inherits the settings default. */
  modelId: string | null
  effort: string | null
  defaultModelId: string
  defaultEffort: string | null
  onModelChange: (model: string | null) => void
  onEffortChange: (effort: string | null) => void
}

function modelLabel(models: CatalogModel[], id: string | null, fallback: string): string {
  if (!id) {
    return fallback
  }
  return models.find((model) => model.id === id)?.displayName ?? id
}

function effortLabel(value: string | null): string {
  return EFFORTS.find((effort) => effort.value === value)?.label ?? 'Default'
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

export function Composer({
  disabled,
  phase,
  onSubmit,
  placement = 'dock',
  models,
  modelId,
  effort,
  defaultModelId,
  defaultEffort,
  onModelChange,
  onEffortChange
}: ComposerProps) {
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

  const resolvedModel = modelId ?? defaultModelId
  const resolvedEffort = effort ?? defaultEffort
  const modelOptions = models.map((model) => ({ value: model.id, label: model.displayName }))
  if (resolvedModel && !modelOptions.some((option) => option.value === resolvedModel)) {
    modelOptions.unshift({ value: resolvedModel, label: resolvedModel })
  }

  const controls = (
    <div className={styles.cluster}>
      <ChoiceMenu
        label={modelLabel(models, resolvedModel, 'Model')}
        ariaLabel="Model"
        value={modelId ?? ''}
        options={modelOptions}
        onSelect={onModelChange}
        triggerClassName={styles.control}
      />
      <ChoiceMenu
        label={effortLabel(resolvedEffort)}
        ariaLabel="Reasoning effort"
        value={effort ?? ''}
        options={EFFORTS}
        onSelect={onEffortChange}
        triggerClassName={styles.control}
      />
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
