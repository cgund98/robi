import { useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import { Paperclip } from 'lucide-react'

import type { CatalogModel } from '../../api/models'
import type { ChatMessage } from '../../api/messages'
import type { AgentMode } from '../../api/sessions'
import { ChoiceMenu } from './ChoiceMenu'
import { ModelEffortMenu } from './ModelEffortMenu'
import { SkillMenu } from './SkillMenu'
import { ContextMeter } from './ContextMeter'
import {
  readComposerDraft,
  subscribeComposerDrafts,
  writeComposerDraft
} from '../../state/composerDrafts'
import {
  fileAccept,
  filesFromTransfer,
  instructionWithTextFiles,
  isImageFile,
  isTextFile,
  MAX_ATTACHMENTS,
  readTextFile
} from './textAttachments'
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
  /** Block send. The field stays editable. */
  disabled: boolean
  /** The send request has not returned yet. */
  pending?: boolean
  /** The session actor is running. Send becomes stop. */
  running?: boolean
  /** Stop has been requested and the actor has not exited yet. */
  stopping?: boolean
  onStop?: () => void
  onSubmit: (text: string, images?: File[]) => Promise<boolean>
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
  /** Session id, or `draft` for a chat that has no row yet. */
  draftKey: string
  /** Instruction echoed in the transcript before the stored user row exists. */
  pendingText?: string | null
  workspaceId?: string | null
}

function hasFiles(data: DataTransfer): boolean {
  return Array.from(data.types).includes('Files')
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
  pending = false,
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
  draftKey,
  pendingText = null,
  workspaceId = null
}: ComposerProps) {
  const draft = useSyncExternalStore(
    subscribeComposerDrafts,
    () => readComposerDraft(draftKey),
    () => ''
  )
  const [boundKey, setBoundKey] = useState(draftKey)
  const [caret, setCaret] = useState(0)
  const [images, setImages] = useState<File[]>([])
  const [textFiles, setTextFiles] = useState<File[]>([])
  const [attachError, setAttachError] = useState<string | null>(null)
  if (draftKey !== boundKey) {
    setBoundKey(draftKey)
    setCaret(0)
    setImages([])
    setTextFiles([])
    setAttachError(null)
  }

  function updateDraft(next: string) {
    writeComposerDraft(draftKey, next)
  }
  const fieldRef = useRef<HTMLTextAreaElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)
  const dragDepth = useRef(0)
  const [dragOver, setDragOver] = useState(false)
  const attachmentCount = images.length + textFiles.length
  const canSend = !disabled && (draft.trim().length > 0 || attachmentCount > 0)
  const welcome = placement === 'welcome'

  useLayoutEffect(() => {
    const field = fieldRef.current
    if (!field) {
      return
    }
    field.style.height = 'auto'
    field.style.height = `${field.scrollHeight}px`
  }, [draft])

  async function submit() {
    if (!canSend) {
      return
    }
    setAttachError(null)
    let text: string
    try {
      const read = await Promise.all(
        textFiles.map(async (file) => ({ name: file.name, text: await readTextFile(file) }))
      )
      text = instructionWithTextFiles(draft, read)
    } catch (error) {
      setAttachError(error instanceof Error ? error.message : 'Could not read that file')
      return
    }
    const sent = await onSubmit(text, images)
    if (sent) {
      updateDraft('')
      setImages([])
      setTextFiles([])
    }
  }

  function onPick(files: FileList | File[] | null) {
    if (!files || disabled) {
      return
    }
    const room = MAX_ATTACHMENTS - images.length - textFiles.length
    if (room <= 0) {
      return
    }
    const picked = Array.from(files).slice(0, room)
    const nextImages = [...images]
    const nextText = [...textFiles]
    const skipped: string[] = []
    for (const file of picked) {
      if (isImageFile(file)) {
        nextImages.push(file)
      } else if (isTextFile(file)) {
        nextText.push(file)
      } else {
        skipped.push(file.name)
      }
    }
    setImages(nextImages)
    setTextFiles(nextText)
    setAttachError(
      skipped.length > 0 ? `${skipped.join(', ')} is not an image or a text file` : null
    )
  }

  const resolvedModel = modelId ?? defaultModelId
  const resolvedEffort = effort ?? defaultEffort
  const contextWindow = models.find((model) => model.id === resolvedModel)?.contextWindow ?? null
  const modelOptions = models.map((model) => ({ value: model.id, label: model.displayName }))
  if (resolvedModel && !modelOptions.some((option) => option.value === resolvedModel)) {
    modelOptions.unshift({ value: resolvedModel, label: resolvedModel })
  }

  const attach = (
    <button
      type="button"
      className={styles.attach}
      disabled={disabled || attachmentCount >= MAX_ATTACHMENTS}
      aria-label="Attach a file"
      onClick={() => fileRef.current?.click()}
    >
      <Paperclip size={16} strokeWidth={1.75} />
    </button>
  )

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
      <ModelEffortMenu
        modelLabel={modelLabel(models, resolvedModel, 'Model')}
        effortLabel={effortLabel(resolvedEffort)}
        modelValue={modelId ?? ''}
        effortValue={effort ?? ''}
        models={modelOptions}
        efforts={EFFORTS}
        onModelSelect={onModelChange}
        onEffortSelect={onEffortChange}
        triggerClassName={`${styles.control} ${styles.modelEffort}`}
      />
      {attach}
      {welcome ? null : (
        <ContextMeter
          messages={messages}
          draft={draft}
          pendingText={pendingText ?? ''}
          contextWindow={contextWindow}
        />
      )}
    </div>
  )

  return (
    <div className={welcome ? styles.welcome : styles.composer}>
      <div className={styles.column}>
        <div
          className={`${welcome ? styles.card : styles.field} ${dragOver ? styles.dropTarget : ''}`}
          onDragEnter={(event) => {
            if (disabled || !hasFiles(event.dataTransfer)) {
              return
            }
            event.preventDefault()
            dragDepth.current += 1
            setDragOver(true)
          }}
          onDragOver={(event) => {
            if (disabled || !hasFiles(event.dataTransfer)) {
              return
            }
            event.preventDefault()
            event.dataTransfer.dropEffect = 'copy'
          }}
          onDragLeave={() => {
            dragDepth.current = Math.max(0, dragDepth.current - 1)
            if (dragDepth.current === 0) {
              setDragOver(false)
            }
          }}
          onDrop={(event) => {
            event.preventDefault()
            dragDepth.current = 0
            setDragOver(false)
            onPick(filesFromTransfer(event.dataTransfer))
          }}
        >
          {attachmentCount > 0 ? (
            <div className={styles.thumbnails}>
              {images.map((image, index) => (
                <div key={`${image.name}-${index}`} className={styles.thumbnail}>
                  <img
                    src={URL.createObjectURL(image)}
                    alt={image.name}
                    className={styles.thumbnailImg}
                  />
                  <button
                    type="button"
                    className={styles.removeThumb}
                    aria-label={`Remove ${image.name}`}
                    onClick={() => setImages(images.filter((_, i) => i !== index))}
                  >
                    ×
                  </button>
                </div>
              ))}
              {textFiles.map((file, index) => (
                <div key={`${file.name}-${index}`} className={styles.fileChip}>
                  <span className={styles.fileName}>{file.name}</span>
                  <button
                    type="button"
                    className={styles.removeChip}
                    aria-label={`Remove ${file.name}`}
                    onClick={() => setTextFiles(textFiles.filter((_, i) => i !== index))}
                  >
                    ×
                  </button>
                </div>
              ))}
            </div>
          ) : null}
          {attachError ? (
            <p className={styles.attachError} role="alert">
              {attachError}
            </p>
          ) : null}
          <div className={welcome ? styles.cardBody : styles.fieldRow}>
            <textarea
              ref={fieldRef}
              className={welcome ? styles.cardInput : styles.input}
              rows={welcome ? 2 : 1}
              placeholder="Describe a task or ask a question"
              value={draft}
              onChange={(event) => {
                updateDraft(event.target.value)
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
              onPaste={(event) => {
                const files = filesFromTransfer(event.clipboardData)
                if (files.length === 0) {
                  return
                }
                event.preventDefault()
                onPick(files)
              }}
              aria-label="Message"
            />
            <SkillMenu
              workspaceId={workspaceId}
              draft={draft}
              caret={caret}
              onInsert={(next, caretNext) => {
                updateDraft(next)
                setCaret(caretNext)
                requestAnimationFrame(() => {
                  fieldRef.current?.focus()
                  fieldRef.current?.setSelectionRange(caretNext, caretNext)
                })
              }}
            />
            <input
              ref={fileRef}
              type="file"
              accept={fileAccept()}
              multiple
              className={styles.fileInput}
              onChange={(event) => {
                onPick(event.target.files)
                event.target.value = ''
              }}
              aria-label="Attach files"
            />
            {welcome ? (
              <div className={styles.cardBar}>
                {controls}
                <SendOrStop
                  canSend={canSend}
                  pending={pending}
                  running={running}
                  stopping={stopping}
                  onSend={() => void submit()}
                  onStop={onStop}
                />
              </div>
            ) : (
              <SendOrStop
                canSend={canSend}
                pending={pending}
                running={running}
                stopping={stopping}
                onSend={() => void submit()}
                onStop={onStop}
              />
            )}
          </div>
        </div>

        {welcome ? null : <div className={styles.toolbar}>{controls}</div>}
      </div>
    </div>
  )
}

function StopIcon() {
  return (
    <svg className={styles.stopIcon} viewBox="0 0 16 16" aria-hidden>
      <path
        fill="currentColor"
        fillRule="evenodd"
        d="M8 1.25a6.75 6.75 0 1 0 .001 13.5A6.75 6.75 0 0 0 8 1.25ZM6.4 5.25h3.2a1.15 1.15 0 0 1 1.15 1.15v3.2a1.15 1.15 0 0 1-1.15 1.15h-3.2a1.15 1.15 0 0 1-1.15-1.15v-3.2A1.15 1.15 0 0 1 6.4 5.25Z"
      />
    </svg>
  )
}

function SendOrStop({
  canSend,
  pending,
  running,
  stopping,
  onSend,
  onStop
}: {
  canSend: boolean
  pending: boolean
  running: boolean
  stopping: boolean
  onSend: () => void
  onStop?: () => void
}) {
  if (pending && !running) {
    return <span className={styles.pending} role="status" aria-label="Sending" />
  }

  if (running) {
    return (
      <button
        type="button"
        className={styles.send}
        disabled={stopping}
        aria-label="Stop"
        onClick={onStop}
      >
        <StopIcon />
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
