import { useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'

import type { ReviewFile, ReviewLine } from '../../api/review'
import { AssistantMarkdown } from '../chat/AssistantMarkdown'
import { expandReviewLines, type ReviewView } from './diffView'
import { DiffRows } from './DiffRows'
import diffStyles from './DiffList.module.css'
import type { PaintedToken } from './highlight'
import styles from './FileViewDialog.module.css'

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

type FileViewDialogProps = {
  file: ReviewFile | null
  baseline: PaintedToken[][]
  current: PaintedToken[][]
  onClose: () => void
}

export function FileViewDialog({ file, baseline, current, onClose }: FileViewDialogProps) {
  return (
    <Dialog.Root
      open={file !== null}
      onOpenChange={(next) => {
        if (!next) {
          onClose()
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.panel}>
          {file ? (
            <FileView key={file.path} file={file} baseline={baseline} current={current} />
          ) : null}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function FileView({
  file,
  baseline,
  current
}: {
  file: ReviewFile
  baseline: PaintedToken[][]
  current: PaintedToken[][]
}) {
  const [view, setView] = useState<FileMode>('diff')
  const markdown = isMarkdown(file.path)
  const modes = markdown ? VIEWS : VIEWS.filter((item) => item.id !== 'preview')
  const sides = { baseline, current }
  const diffLines = expandReviewLines(file.lines, file.baseline, file.current)
  const previewText = file.status === 'deleted' ? file.baseline : file.current
  const note =
    view === 'previous' && file.status === 'added'
      ? 'File added'
      : view === 'current' && file.status === 'deleted'
        ? 'File deleted'
        : null
  const whole = view === 'previous' ? baseline : view === 'current' ? current : null

  return (
    <>
      <header className={styles.head}>
        <Dialog.Title className={styles.title}>{file.path}</Dialog.Title>
        <div className={styles.views} role="radiogroup" aria-label="File version">
          {modes.map((item) => (
            <button
              key={item.id}
              type="button"
              role="radio"
              aria-checked={view === item.id}
              className={view === item.id ? styles.viewActive : styles.view}
              onClick={() => setView(item.id)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <Dialog.Close className={styles.close}>Close</Dialog.Close>
      </header>
      <Dialog.Description className={styles.srOnly}>
        {note ??
          (view === 'preview'
            ? `Preview of ${file.path}`
            : view === 'diff'
              ? `Diff of ${file.path}`
              : view === 'previous'
                ? `Previous contents of ${file.path}`
                : `Current contents of ${file.path}`)}
      </Dialog.Description>
      <div className={view === 'preview' ? styles.bodyPreview : styles.body}>
        {note ? (
          <p className={styles.empty}>{note}</p>
        ) : view === 'preview' ? (
          previewText === '' ? (
            <p className={styles.empty}>This file is empty.</p>
          ) : (
            <AssistantMarkdown text={previewText} document />
          )
        ) : view === 'diff' ? (
          <div className={diffStyles.wide}>
            <DiffRows lines={diffLines} tokensForLine={(line) => tokensFor(line, sides)} />
          </div>
        ) : whole && whole.length === 0 ? (
          <p className={styles.empty}>This file is empty.</p>
        ) : (
          <div className={styles.sheet}>
            <div className={styles.gutter} aria-hidden="true">
              {whole?.map((_, index) => (
                <div key={index} className={styles.num}>
                  {index + 1}
                </div>
              ))}
            </div>
            <div className={styles.codeCol}>
              {whole?.map((tokens, index) => (
                <div key={index} className={styles.code}>
                  {tokens.every((token) => token.text === '')
                    ? '\u00a0'
                    : tokens.map((token, tokenIndex) => (
                        <span
                          key={tokenIndex}
                          style={token.color ? { color: token.color } : undefined}
                        >
                          {token.text}
                        </span>
                      ))}
                </div>
              ))}
            </div>
          </div>
        )}
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
