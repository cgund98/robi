/** @jsxImportSource solid-js */
import { Dialog } from '@kobalte/core/dialog'
import { createSignal, For, Show } from 'solid-js'

import type { ReviewFile, ReviewLine } from '../../api/review'
import { expandReviewLines, type ReviewView } from './diffView'
import type { PaintedToken } from './highlight'
import diffStyles from './DiffList.module.css'
import styles from './FileViewDialog.module.css'
import { AssistantMarkdown } from '../chat/AssistantMarkdown'
import { DiffRows } from './DiffRows'

type FileMode = ReviewView | 'preview'

const VIEWS: { id: FileMode; label: string }[] = [
  { id: 'diff', label: 'Diff' },
  { id: 'current', label: 'Current' },
  { id: 'previous', label: 'Previous' },
  { id: 'preview', label: 'Preview' }
]

function isMarkdown(path: string): boolean {
  const name = path.split('/').pop() ?? path
  const dot = name.lastIndexOf('.')
  if (dot < 0) {
    return false
  }
  const ext = name.slice(dot + 1).toLowerCase()
  return ext === 'md' || ext === 'markdown'
}

export function FileViewDialog(props: {
  file: ReviewFile | null
  baseline: PaintedToken[][]
  current: PaintedToken[][]
  onClose: () => void
}) {
  return (
    <Dialog
      open={props.file !== null}
      onOpenChange={(next) => {
        if (!next) {
          props.onClose()
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay class={styles.overlay} />
        <Dialog.Content class={styles.panel}>
          <Show when={props.file} keyed>
            {(file) => <FileView file={file} baseline={props.baseline} current={props.current} />}
          </Show>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog>
  )
}

function FileView(props: {
  file: ReviewFile
  baseline: PaintedToken[][]
  current: PaintedToken[][]
}) {
  const [view, setView] = createSignal<FileMode>('diff')
  const markdown = () => isMarkdown(props.file.path)
  const modes = () => (markdown() ? VIEWS : VIEWS.filter((item) => item.id !== 'preview'))
  const sides = () => ({ baseline: props.baseline, current: props.current })
  const diffLines = () =>
    expandReviewLines(props.file.lines, props.file.baseline, props.file.current)
  const previewText = () =>
    props.file.status === 'deleted' ? props.file.baseline : props.file.current
  const note = () =>
    view() === 'previous' && props.file.status === 'added'
      ? 'File added'
      : view() === 'current' && props.file.status === 'deleted'
        ? 'File deleted'
        : null
  const whole = () =>
    view() === 'previous' ? props.baseline : view() === 'current' ? props.current : null
  const description = () =>
    note() ??
    (view() === 'preview'
      ? `Preview of ${props.file.path}`
      : view() === 'diff'
        ? `Diff of ${props.file.path}`
        : view() === 'previous'
          ? `Previous contents of ${props.file.path}`
          : `Current contents of ${props.file.path}`)

  return (
    <>
      <header class={styles.head}>
        <Dialog.Title class={styles.title}>{props.file.path}</Dialog.Title>
        <div class={styles.views} role="radiogroup" aria-label="File version">
          <For each={modes()}>
            {(item) => (
              <button
                type="button"
                role="radio"
                aria-checked={view() === item.id}
                class={view() === item.id ? styles.viewActive : styles.view}
                onClick={() => setView(item.id)}
              >
                {item.label}
              </button>
            )}
          </For>
        </div>
        <Dialog.CloseButton class={styles.close}>Close</Dialog.CloseButton>
      </header>
      <Dialog.Description class={styles.srOnly}>{description()}</Dialog.Description>
      <div class={view() === 'preview' ? styles.bodyPreview : styles.body}>
        <Show when={!note()} fallback={<p class={styles.empty}>{note()}</p>}>
          <Show
            when={view() !== 'preview'}
            fallback={
              <Show
                when={previewText() !== ''}
                fallback={<p class={styles.empty}>This file is empty.</p>}
              >
                <AssistantMarkdown text={previewText()} document />
              </Show>
            }
          >
            <Show
              when={view() === 'diff'}
              fallback={
                <Show
                  when={(whole()?.length ?? 0) > 0}
                  fallback={<p class={styles.empty}>This file is empty.</p>}
                >
                  <div class={styles.sheet}>
                    <div class={styles.gutter} aria-hidden="true">
                      <For each={whole() ?? []}>
                        {(_, index) => <div class={styles.num}>{index() + 1}</div>}
                      </For>
                    </div>
                    <div class={styles.codeCol}>
                      <For each={whole() ?? []}>
                        {(tokens) => (
                          <div class={styles.code}>
                            {tokens.every((token) => token.text === '')
                              ? '\u00a0'
                              : tokens.map((token) => (
                                  <span style={token.color ? { color: token.color } : undefined}>
                                    {token.text}
                                  </span>
                                ))}
                          </div>
                        )}
                      </For>
                    </div>
                  </div>
                </Show>
              }
            >
              <div class={diffStyles.wide}>
                <DiffRows lines={diffLines()} tokensForLine={(line) => tokensFor(line, sides())} />
              </div>
            </Show>
          </Show>
        </Show>
      </div>
    </>
  )
}

function tokensFor(
  line: ReviewLine,
  sides: { baseline: PaintedToken[][]; current: PaintedToken[][] }
): PaintedToken[] {
  if (line.kind === 'gap') {
    return [{ text: line.text }]
  }
  const source = line.kind === 'delete' ? sides.baseline : sides.current
  const number = line.kind === 'delete' ? line.old_line : line.new_line
  if (number == null) {
    return [{ text: line.text }]
  }
  return source[number - 1] ?? [{ text: line.text }]
}
