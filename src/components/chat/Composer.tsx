import { useState } from 'react'

import type { CatalogModel } from '../../api/models'
import type { ChatMessage } from '../../api/messages'
import type { AgentMode } from '../../api/sessions'
import { ChoiceMenu } from './ChoiceMenu'
import { ContextMeter } from './ContextMeter'
import styles from './Composer.module.css'

const MODES: { value: AgentMode; label: string }[] = [
  { value: 'ask', label: 'Ask' },
  { value: 'plan', label: 'Plan' },
  { value: 'agent', label: 'Agent' }
]

const EFFORTS = [
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' }
]

type ComposerProps = {
  disabled: boolean
  onSubmit: (text: string) => Promise<boolean>
  /** Centered card on an empty chat. Dock keeps the field at the bottom of a thread. */
  placement?: 'dock' | 'welcome'
  models: CatalogModel[]
  mode: AgentMode
  /** Session override for the active mode. Null inherits that mode's setting. */
  modelId: string | null
  effort: string | null
  defaultModelId: string
  defaultEffort: string | null
  onModeChange: (mode: AgentMode) => void
  onModelChange: (model: string | null) => void
  onEffortChange: (effort: string | null) => void
  messages: ChatMessage[]
  /** Instruction echoed in the transcript before the stored user row exists. */
  pendingText?: string | null
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

export function Composer({
  disabled,
  onSubmit,
  placement = 'dock',
  models,
  mode,
  modelId,
  effort,
  defaultModelId,
  defaultEffort,
  onModeChange,
  onModelChange,
  onEffortChange,
  messages,
  pendingText = null
}: ComposerProps) {
  const [draft, setDraft] = useState('')
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
  const contextWindow = models.find((model) => model.id === resolvedModel)?.contextWindow ?? null
  const modelOptions = models.map((model) => ({ value: model.id, label: model.displayName }))
  if (resolvedModel && !modelOptions.some((option) => option.value === resolvedModel)) {
    modelOptions.unshift({ value: resolvedModel, label: resolvedModel })
  }

  const controls = (
    <div className={styles.cluster}>
      <ChoiceMenu
        label={MODES.find((item) => item.value === mode)?.label ?? 'Agent'}
        ariaLabel="Mode"
        value={mode}
        options={MODES}
        includeDefault={false}
        onSelect={(value) => {
          if (value === 'ask' || value === 'plan' || value === 'agent') {
            onModeChange(value)
          }
        }}
        triggerClassName={styles.control}
      />
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
      <ContextMeter
        messages={messages}
        draft={draft}
        pendingText={pendingText ?? ''}
        contextWindow={contextWindow}
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
