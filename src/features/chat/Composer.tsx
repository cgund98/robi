/** @jsxImportSource solid-js */
import { PaperClip } from '../../components/ui/icons'
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'

import { modelDisplayName, type CatalogModel } from '../../api/models'
import type { ChatMessage } from '../../api/messages'
import type { AgentMode } from '../../api/sessions'
import { pickAttachmentFiles, type PickedAttachment } from '../../infra/pickAttachmentFiles'
import {
  readComposerDraft,
  subscribeComposerDrafts,
  writeComposerDraft
} from '../../state/composerDrafts'
import {
  subscribeComposerAttachments,
  takeComposerAttachments
} from '../../state/composerAttachments'
import styles from './Composer.module.css'
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
import { ChoiceMenu } from './ChoiceMenu'
import { ContextMeter } from './ContextMeter'
import { ModelEffortMenu } from './ModelEffortMenu'
import { SkillMenu } from './SkillMenu'

const MODES: { value: AgentMode; label: string; tone: AgentMode }[] = [
  { value: 'ask', label: 'Ask', tone: 'ask' },
  { value: 'plan', label: 'Plan', tone: 'plan' },
  { value: 'agent', label: 'Agent', tone: 'agent' }
]

const EFFORTS = [
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' }
]

function hasFiles(data: DataTransfer): boolean {
  return Array.from(data.types).includes('Files')
}

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

export function Composer(props: {
  disabled: boolean
  pending?: boolean
  running?: boolean
  stopping?: boolean
  onStop?: () => void
  onSubmit: (text: string, images?: File[], files?: FileAttachment[]) => Promise<boolean>
  placement?: 'dock' | 'welcome'
  models: CatalogModel[]
  mode: AgentMode
  modelId: string | null
  effort: string | null
  defaultModelId: string
  defaultEffort: string | null
  onModeChange: (mode: AgentMode) => void
  onModelChange: (model: string | null) => void
  onEffortChange: (effort: string | null) => void
  messages: ChatMessage[]
  draftKey: string
  pendingText?: string | null
  workspaceId?: string | null
  onCompact?: () => void
  compacting?: boolean
}) {
  const [draft, setDraft] = createSignal(readComposerDraft(props.draftKey))
  const [caret, setCaret] = createSignal(0)
  const [images, setImages] = createSignal<File[]>([])
  const [files, setFiles] = createSignal<FileAttachment[]>([])
  const [attachError, setAttachError] = createSignal<string | null>(null)
  const [dragOver, setDragOver] = createSignal(false)
  let fieldRef: HTMLTextAreaElement | undefined
  let fileRef: HTMLInputElement | undefined
  let dragDepth = 0
  let filesNow = files()
  let imagesNow = images()

  createEffect(() => {
    filesNow = files()
    imagesNow = images()
  })

  // Reading `props.draftKey` subscribes to the whole spread of chat props, so
  // these effects also run when the mode, model, or transcript changes. Only a
  // new draft key should reload the text and drop attachments.
  let seenDraftKey: string | undefined
  createEffect(() => {
    const key = props.draftKey
    onCleanup(subscribeComposerDrafts(() => setDraft(readComposerDraft(key))))
    if (key === seenDraftKey) {
      return
    }
    seenDraftKey = key
    setDraft(readComposerDraft(key))
    setCaret(0)
    setImages([])
    setFiles([])
    setAttachError(null)
  })

  createEffect(() => {
    const key = props.draftKey
    const drain = () => {
      const incoming = takeComposerAttachments(key)
      if (incoming.length === 0) {
        return
      }
      const errors: string[] = []
      setFiles(appendAttachments(filesNow, imagesNow.length, incoming, errors))
      if (errors.length > 0) {
        setAttachError(errors.join(', '))
      }
      requestAnimationFrame(() => fieldRef?.focus())
    }
    drain()
    onCleanup(subscribeComposerAttachments(drain))
  })

  createEffect(() => {
    draft()
    const field = fieldRef
    if (!field) {
      return
    }
    field.style.height = 'auto'
    field.style.height = `${field.scrollHeight}px`
  })

  function updateDraft(next: string) {
    writeComposerDraft(props.draftKey, next)
  }

  const welcome = () => (props.placement ?? 'dock') === 'welcome'
  const attachmentCount = () => images().length + files().length
  const canSend = () => !props.disabled && (draft().trim().length > 0 || attachmentCount() > 0)

  async function submit() {
    if (!canSend()) {
      return
    }
    setAttachError(null)
    const text = draft()
    const sentImages = images()
    const sentFiles = files()
    updateDraft('')
    setImages([])
    setFiles([])
    let sent = false
    try {
      sent = await props.onSubmit(text, sentImages, sentFiles)
    } catch {
      sent = false
    }
    if (sent) {
      return
    }
    if (draft().length === 0) {
      updateDraft(text)
    }
    if (images().length === 0) {
      setImages(sentImages)
    }
    if (files().length === 0) {
      setFiles(sentFiles)
    }
  }

  async function onPick(list: FileList | File[] | null) {
    if (!list || props.disabled) {
      return
    }
    const room = MAX_ATTACHMENTS - images().length - files().length
    if (room <= 0) {
      return
    }
    const picked = Array.from(list).slice(0, room)
    const nextImages = [...images()]
    const textFiles: FileAttachment[] = []
    const skipped: string[] = []
    for (const file of picked) {
      if (isImageFile(file)) {
        nextImages.push(file)
        continue
      }
      try {
        textFiles.push(await readFileAttachment(file))
      } catch (error) {
        skipped.push(error instanceof Error ? error.message : `${file.name} could not be read`)
      }
    }
    const errors: string[] = []
    const nextFiles = appendAttachments(files(), nextImages.length, textFiles, errors)
    setImages(nextImages)
    setFiles(nextFiles)
    setAttachError(
      errors.length > 0 ? errors.join(', ') : skipped.length > 0 ? skipped.join(', ') : null
    )
  }

  function addPicked(picked: PickedAttachment[]) {
    const room = MAX_ATTACHMENTS - images().length - files().length
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
    setFiles(appendAttachments(files(), images().length, incoming, errors))
    setAttachError(errors.length > 0 ? errors.join(', ') : null)
  }

  async function onAttach() {
    let picked: PickedAttachment[] | null
    try {
      picked = await pickAttachmentFiles()
    } catch (error) {
      setAttachError(error instanceof Error ? error.message : 'Could not open the file picker')
      return
    }
    if (picked === null) {
      fileRef?.click()
      return
    }
    addPicked(picked)
  }

  const resolvedModel = () => props.modelId ?? props.defaultModelId
  const resolvedEffort = () => props.effort ?? props.defaultEffort
  const contextWindow = () =>
    props.models.find((model) => model.id === resolvedModel())?.contextWindow ?? null
  const modelOptions = () => {
    const options = props.models.map((model) => ({ value: model.id, label: model.displayName }))
    const selected = resolvedModel()
    if (selected && !options.some((option) => option.value === selected)) {
      options.unshift({ value: selected, label: modelDisplayName(selected) })
    }
    return options
  }
  const modeClass = () =>
    props.mode === 'ask' ? styles.modeAsk : props.mode === 'plan' ? styles.modePlan : ''

  const controls = () => (
    <div class={styles.cluster}>
      <ChoiceMenu
        label={MODES.find((item) => item.value === props.mode)?.label ?? 'Agent'}
        ariaLabel="Mode"
        value={props.mode}
        options={MODES}
        includeDefault={false}
        onSelect={(value) => {
          if (value === 'ask' || value === 'plan' || value === 'agent') {
            props.onModeChange(value)
          }
        }}
        triggerClassName={`${styles.control} ${styles.mode} ${modeClass()}`}
      />
      <ModelEffortMenu
        modelLabel={modelLabel(props.models, resolvedModel(), 'Model')}
        effortLabel={effortLabel(resolvedEffort())}
        modelValue={props.modelId ?? ''}
        effortValue={props.effort ?? ''}
        models={modelOptions()}
        efforts={EFFORTS}
        onModelSelect={props.onModelChange}
        onEffortSelect={props.onEffortChange}
        triggerClassName={`${styles.control} ${styles.modelEffort}`}
      />
      <button
        type="button"
        class={styles.attach}
        disabled={props.disabled || attachmentCount() >= MAX_ATTACHMENTS}
        aria-label="Attach a file"
        onClick={() => void onAttach()}
      >
        <PaperClip size={16} />
      </button>
      <Show when={!welcome()}>
        <ContextMeter
          messages={props.messages}
          draft={draft()}
          pendingText={props.pendingText ?? ''}
          contextWindow={contextWindow()}
          onCompact={props.onCompact}
          compacting={props.compacting}
          compactDisabled={props.running || props.disabled}
        />
      </Show>
    </div>
  )

  return (
    <div class={welcome() ? styles.welcome : styles.composer}>
      <div class={styles.column}>
        <div
          class={`${welcome() ? styles.card : styles.field} ${dragOver() ? styles.dropTarget : ''}`}
          onDragEnter={(event) => {
            const transfer = event.dataTransfer
            if (props.disabled || !transfer || !hasFiles(transfer)) {
              return
            }
            event.preventDefault()
            dragDepth += 1
            setDragOver(true)
          }}
          onDragOver={(event) => {
            const transfer = event.dataTransfer
            if (props.disabled || !transfer || !hasFiles(transfer)) {
              return
            }
            event.preventDefault()
            transfer.dropEffect = 'copy'
          }}
          onDragLeave={() => {
            dragDepth = Math.max(0, dragDepth - 1)
            if (dragDepth === 0) {
              setDragOver(false)
            }
          }}
          onDrop={(event) => {
            event.preventDefault()
            dragDepth = 0
            setDragOver(false)
            void onPick(event.dataTransfer ? filesFromTransfer(event.dataTransfer) : [])
          }}
        >
          <Show when={attachmentCount() > 0}>
            <div class={styles.thumbnails}>
              <For each={images()}>
                {(image, index) => (
                  <div class={styles.thumbnail}>
                    <img
                      src={URL.createObjectURL(image)}
                      alt={image.name}
                      class={styles.thumbnailImg}
                    />
                    <button
                      type="button"
                      class={styles.removeThumb}
                      aria-label={`Remove ${image.name}`}
                      onClick={() => setImages(images().filter((_, i) => i !== index()))}
                    >
                      ×
                    </button>
                  </div>
                )}
              </For>
              <For each={files()}>
                {(file, index) => (
                  <AttachmentChip
                    name={file.name}
                    path={file.path ?? file.absolutePath}
                    startLine={file.startLine}
                    endLine={file.endLine}
                    onRemove={() => setFiles(files().filter((_, i) => i !== index()))}
                  />
                )}
              </For>
            </div>
          </Show>
          <Show when={attachError()}>
            <p class={styles.attachError} role="alert">
              {attachError()}
            </p>
          </Show>
          <div class={welcome() ? styles.cardBody : styles.fieldRow}>
            <textarea
              ref={fieldRef}
              class={welcome() ? styles.cardInput : styles.input}
              rows={welcome() ? 2 : 1}
              placeholder="Describe a task or ask a question"
              value={draft()}
              onInput={(event) => {
                updateDraft(event.currentTarget.value)
                setCaret(event.currentTarget.selectionStart)
              }}
              onSelect={(event) => setCaret(event.currentTarget.selectionStart)}
              onKeyUp={(event) => setCaret(event.currentTarget.selectionStart)}
              onKeyDown={(event) => {
                if (event.key !== 'Enter' || event.shiftKey || event.isComposing) {
                  return
                }
                event.preventDefault()
                void submit()
              }}
              onPaste={(event) => {
                const pasted = filesFromTransfer(event.clipboardData)
                if (pasted.length === 0) {
                  return
                }
                event.preventDefault()
                void onPick(pasted)
              }}
              aria-label="Message"
            />
            <SkillMenu
              workspaceId={props.workspaceId ?? null}
              draft={draft()}
              caret={caret()}
              onInsert={(next, caretNext) => {
                updateDraft(next)
                setCaret(caretNext)
                requestAnimationFrame(() => {
                  fieldRef?.focus()
                  fieldRef?.setSelectionRange(caretNext, caretNext)
                })
              }}
            />
            <input
              ref={fileRef}
              type="file"
              accept={fileAccept()}
              multiple
              class={styles.fileInput}
              onChange={(event) => {
                void onPick(event.currentTarget.files)
                event.currentTarget.value = ''
              }}
              aria-label="Attach files"
            />
            <Show
              when={welcome()}
              fallback={
                <SendOrStop
                  canSend={canSend()}
                  pending={props.pending ?? false}
                  running={props.running ?? false}
                  stopping={props.stopping ?? false}
                  onSend={() => void submit()}
                  onStop={props.onStop}
                />
              }
            >
              <div class={styles.cardBar}>
                {controls()}
                <SendOrStop
                  canSend={canSend()}
                  pending={props.pending ?? false}
                  running={props.running ?? false}
                  stopping={props.stopping ?? false}
                  onSend={() => void submit()}
                  onStop={props.onStop}
                />
              </div>
            </Show>
          </div>
        </div>
        <Show when={!welcome()}>
          <div class={styles.toolbar}>{controls()}</div>
        </Show>
      </div>
    </div>
  )
}

function StopIcon() {
  return (
    <svg class={styles.stopIcon} viewBox="0 0 16 16" aria-hidden="true">
      <path
        fill="currentColor"
        fill-rule="evenodd"
        d="M8 1.25a6.75 6.75 0 1 0 .001 13.5A6.75 6.75 0 0 0 8 1.25ZM6.4 5.25h3.2a1.15 1.15 0 0 1 1.15 1.15v3.2a1.15 1.15 0 0 1-1.15 1.15h-3.2a1.15 1.15 0 0 1-1.15-1.15v-3.2A1.15 1.15 0 0 1 6.4 5.25Z"
      />
    </svg>
  )
}

function SendOrStop(props: {
  canSend: boolean
  pending: boolean
  running: boolean
  stopping: boolean
  onSend: () => void
  onStop?: () => void
}) {
  return (
    <Show
      when={!(props.pending && !props.running)}
      fallback={<span class={styles.pending} role="status" aria-label="Sending" />}
    >
      <Show
        when={props.running}
        fallback={
          <button
            type="button"
            class={styles.send}
            disabled={!props.canSend}
            aria-label="Send"
            onClick={() => props.onSend()}
          >
            ⏎
          </button>
        }
      >
        <button
          type="button"
          class={styles.send}
          disabled={props.stopping}
          aria-label="Stop"
          onClick={() => props.onStop?.()}
        >
          <StopIcon />
        </button>
      </Show>
    </Show>
  )
}
