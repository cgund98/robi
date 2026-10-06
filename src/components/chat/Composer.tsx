import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import { Paperclip } from 'lucide-react'

import { modelDisplayName, type CatalogModel } from '../../api/models'
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
  subscribeComposerAttachments,
  takeComposerAttachments
} from '../../state/composerAttachments'
import {
  attachmentFromPicked,
  fileAccept,
  filesFromTransfer,
  isImageFile,
  MAX_ATTACHMENTS,
  MAX_TEXT_TOTAL_BYTES,
  readFileAttachment,
  type FileAttachment
} from './textAttachments'
import { AttachmentChip } from './AttachmentChip'
import { pickAttachmentFiles, type PickedAttachment } from '../../infra/pickAttachmentFiles'
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
  onSubmit: (text: string, images?: File[], files?: FileAttachment[]) => Promise<boolean>
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
  /** Summarize the older prefix. Omitted where there is no session to compact. */
  onCompact?: () => void
  /** A compact request is in flight. */
  compacting?: boolean
}

function hasFiles(data: DataTransfer): boolean {
  return Array.from(data.types).includes('Files')
}

/**
 * Add attachments to the composer's list, honouring the count and total caps.
 *
 * The one place a new attachment enters the list, so a picker, a drop, and a
 * request from the docs viewer all enforce the same limits. `errors` collects a
 * message per dropped attachment; entries that fit are appended in order.
 */
function appendAttachments(
  current: FileAttachment[],
  imageCount: number,
  incoming: FileAttachment[],
  errors: string[]
): FileAttachment[] {
  const next = [...current]
  for (const attachment of incoming) {
    if (imageCount + next.length >= MAX_ATTACHMENTS) {
      errors.push(`${attachment.name} was not added: up to ${MAX_ATTACHMENTS} attachments`)
      continue
    }
    const total = next.reduce((sum, file) => sum + file.size, 0) + attachment.size
    if (total > MAX_TEXT_TOTAL_BYTES) {
      errors.push(`${attachment.name} would push the attachments over the total limit`)
      continue
    }
    next.push(attachment)
  }
  return next
}

function modelLabel(models: CatalogModel[], id: string | null, fallback: string): string {
  if (!id) {
    return fallback
  }
  return models.find((model) => model.id === id)?.displayName ?? modelDisplayName(id)
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
  workspaceId = null,
  onCompact,
  compacting = false
}: ComposerProps) {
  const draft = useSyncExternalStore(
    subscribeComposerDrafts,
    () => readComposerDraft(draftKey),
    () => ''
  )
  const [boundKey, setBoundKey] = useState(draftKey)
  const [caret, setCaret] = useState(0)
  const [images, setImages] = useState<File[]>([])
  const [files, setFiles] = useState<FileAttachment[]>([])
  const [attachError, setAttachError] = useState<string | null>(null)
  if (draftKey !== boundKey) {
    setBoundKey(draftKey)
    setCaret(0)
    setImages([])
    setFiles([])
    setAttachError(null)
  }

  function updateDraft(next: string) {
    writeComposerDraft(draftKey, next)
  }
  const fieldRef = useRef<HTMLTextAreaElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)
  const dragDepth = useRef(0)
  const [dragOver, setDragOver] = useState(false)
  const attachmentCount = images.length + files.length
  const canSend = !disabled && (draft.trim().length > 0 || attachmentCount > 0)
  const welcome = placement === 'welcome'

  // Latest files and images, for the drain effect below to read without listing
  // them as dependencies (a drain must not re-run when the list changes).
  const filesRef = useRef(files)
  const imagesRef = useRef(images)
  useEffect(() => {
    filesRef.current = files
    imagesRef.current = images
  }, [files, images])

  // A request from the docs viewer's line attach waits here until this composer
  // is the one for `draftKey` — the tray unmounts the composer while closed, so
  // the request can arrive before it mounts. Draining on mount and on each
  // request keeps the field's own state the single source of truth.
  const [attachSignal, setAttachSignal] = useState(0)
  useEffect(() => subscribeComposerAttachments(() => setAttachSignal((count) => count + 1)), [])
  useEffect(() => {
    const incoming = takeComposerAttachments(draftKey)
    if (incoming.length === 0) {
      return
    }
    const errors: string[] = []
    setFiles(appendAttachments(filesRef.current, imagesRef.current.length, incoming, errors))
    if (errors.length > 0) {
      setAttachError(errors.join(', '))
    }
    // The intent is to write about the line, so put the caret in the field.
    requestAnimationFrame(() => fieldRef.current?.focus())
  }, [draftKey, attachSignal])

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
    const sent = await onSubmit(draft, images, files)
    if (sent) {
      updateDraft('')
      setImages([])
      setFiles([])
    }
  }

  async function onPick(list: FileList | File[] | null) {
    if (!list || disabled) {
      return
    }
    const room = MAX_ATTACHMENTS - images.length - files.length
    if (room <= 0) {
      return
    }
    const picked = Array.from(list).slice(0, room)
    const nextImages = [...images]
    const textFiles: FileAttachment[] = []
    const skipped: string[] = []
    for (const file of picked) {
      if (isImageFile(file)) {
        nextImages.push(file)
        continue
      }
      // Not an image: the content decides whether it is text. A binary file is
      // refused here; the server re-checks the same bytes.
      try {
        textFiles.push(await readFileAttachment(file))
      } catch (error) {
        skipped.push(error instanceof Error ? error.message : `${file.name} could not be read`)
      }
    }
    const errors: string[] = []
    const nextFiles = appendAttachments(files, nextImages.length, textFiles, errors)
    setImages(nextImages)
    setFiles(nextFiles)
    setAttachError(
      errors.length > 0 ? errors.join(', ') : skipped.length > 0 ? skipped.join(', ') : null
    )
  }

  /** Attach files the native desktop picker returned, which carry a path. */
  function addPicked(picked: PickedAttachment[]) {
    const room = MAX_ATTACHMENTS - images.length - files.length
    if (room <= 0 || picked.length === 0) {
      return
    }
    const incoming: FileAttachment[] = []
    const errors: string[] = []
    for (const item of picked.slice(0, room)) {
      try {
        incoming.push(attachmentFromPicked(item))
      } catch (error) {
        errors.push(error instanceof Error ? error.message : `${item.name} could not be read`)
      }
    }
    setFiles(appendAttachments(files, images.length, incoming, errors))
    setAttachError(errors.length > 0 ? errors.join(', ') : null)
  }

  /**
   * The paperclip. In the desktop app the native dialog runs, because it is the
   * only picker that returns an absolute path — which the server needs to decide
   * whether the file is inside the workspace. In a browser it falls back to the
   * `<input>`, which yields a basename only, so the file is treated as outside.
   */
  async function onAttach() {
    let picked: PickedAttachment[] | null
    try {
      picked = await pickAttachmentFiles()
    } catch (error) {
      // Surface a failed picker (a missing command, or a shell error) instead of
      // a click that appears to do nothing.
      setAttachError(error instanceof Error ? error.message : 'Could not open the file picker')
      return
    }
    if (picked === null) {
      fileRef.current?.click()
      return
    }
    addPicked(picked)
  }

  const resolvedModel = modelId ?? defaultModelId
  const resolvedEffort = effort ?? defaultEffort
  const contextWindow = models.find((model) => model.id === resolvedModel)?.contextWindow ?? null
  const modelOptions = models.map((model) => ({ value: model.id, label: model.displayName }))
  if (resolvedModel && !modelOptions.some((option) => option.value === resolvedModel)) {
    modelOptions.unshift({ value: resolvedModel, label: modelDisplayName(resolvedModel) })
  }

  const attach = (
    <button
      type="button"
      className={styles.attach}
      disabled={disabled || attachmentCount >= MAX_ATTACHMENTS}
      aria-label="Attach a file"
      onClick={() => void onAttach()}
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
          onCompact={onCompact}
          compacting={compacting}
          compactDisabled={running || disabled}
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
            void onPick(filesFromTransfer(event.dataTransfer))
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
              {files.map((file, index) => (
                <AttachmentChip
                  key={`${file.name}-${index}`}
                  name={file.name}
                  path={file.path ?? file.absolutePath}
                  startLine={file.startLine}
                  endLine={file.endLine}
                  onRemove={() => setFiles(files.filter((_, i) => i !== index))}
                />
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
                void onPick(files)
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
                void onPick(event.target.files)
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
