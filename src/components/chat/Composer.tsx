import { useRef, useState } from 'react'

import type { CatalogModel } from '../../api/models'
import type { ChatMessage } from '../../api/messages'
import type { AgentMode } from '../../api/sessions'
import { ChoiceMenu } from './ChoiceMenu'
import { SkillMenu } from './SkillMenu'
import { ContextMeter } from './ContextMeter'
import styles from './Composer.module.css'

const MODES: { value: AgentMode; label: string; tone: AgentMode }[] = [
  { value: 'ask', label: 'Ask', tone: 'ask' },
  { value: 'plan', label: 'Plan', tone: 'plan' },
  { value: 'agent', label: 'Agent', tone: 'agent' }
]

const MODE_CLASS: Record<AgentMode, string | undefined> = {
  ask: styles.modeAsk,
  plan: styles.modePlan,
  agent: undefined
}

const EFFORTS = [
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' }
]

type ComposerProps = {
  disabled: boolean
  /** The session actor is running. Send becomes stop and the field stays locked. */
  running?: boolean
  /** Stop has been requested and the actor has not exited yet. */
  stopping?: boolean
  onStop?: () => void
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
  workspaceId?: string | null
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
  running = false,
  stopping = false,
  onStop,
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
  pendingText = null,
  workspaceId = null
}: ComposerProps) {
  const [draft, setDraft] = useState('')
  const [caret, setCaret] = useState(0)
  const fieldRef = useRef<HTMLTextAreaElement>(null)
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
        align="start"
        onSelect={(value) => {
          if (value === 'ask' || value === 'plan' || value === 'agent') {
            onModeChange(value)
          }
        }}
        triggerClassName={`${styles.control} ${styles.mode} ${MODE_CLASS[mode] ?? ''}`}
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
            ref={fieldRef}
            className={welcome ? styles.cardInput : styles.input}
            rows={welcome ? 2 : 1}
            placeholder="Describe a task or ask a question"
            value={draft}
            disabled={disabled}
            onChange={(event) => {
              setDraft(event.target.value)
              setCaret(event.target.selectionStart)
            }}
            onSelect={(event) => setCaret(event.currentTarget.selectionStart)}
            onKeyUp={(event) => setCaret(event.currentTarget.selectionStart)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault()
                void submit()
              }
            }}
            aria-label="Message"
          />
          <SkillMenu
            workspaceId={workspaceId}
            draft={draft}
            caret={caret}
            onInsert={(next, caretNext) => {
              setDraft(next)
              setCaret(caretNext)
              requestAnimationFrame(() => {
                fieldRef.current?.focus()
                fieldRef.current?.setSelectionRange(caretNext, caretNext)
              })
            }}
          />
          {welcome ? (
            <div className={styles.cardBar}>
              {controls}
              <SendOrStop
                canSend={canSend}
                running={running}
                stopping={stopping}
                onSend={() => void submit()}
                onStop={onStop}
              />
            </div>
          ) : (
            <SendOrStop
              canSend={canSend}
              running={running}
              stopping={stopping}
              onSend={() => void submit()}
              onStop={onStop}
            />
          )}
        </div>

        {welcome ? null : <div className={styles.toolbar}>{controls}</div>}
      </div>
    </div>
  )
}

function SendOrStop({
  canSend,
  running,
  stopping,
  onSend,
  onStop
}: {
  canSend: boolean
  running: boolean
  stopping: boolean
  onSend: () => void
  onStop?: () => void
}) {
  if (running) {
    return (
      <button
        type="button"
        className={styles.send}
        disabled={stopping}
        aria-label="Stop"
        onClick={onStop}
      >
        <svg className={styles.stopIcon} viewBox="0 0 16 16" aria-hidden>
          <rect x="3.5" y="3.5" width="9" height="9" rx="1.5" fill="currentColor" />
        </svg>
      </button>
    )
  }

  return (
    <button
      type="button"
      className={styles.send}
      disabled={!canSend}
      aria-label="Send"
      onClick={onSend}
    >
      ⏎
    </button>
  )
}
