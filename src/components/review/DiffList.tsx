import { Fragment, useEffect, useState } from 'react'

import type { ReviewFile, ReviewLine } from '../../api/review'
import { chunksFor, linesForView, reviewNote, type ReviewView } from './diffView'
import { paintSides, type PaintedToken } from './highlight'
import styles from './DiffList.module.css'

type Sides = {
  baseline: PaintedToken[][]
  current: PaintedToken[][]
}

type DiffListProps = {
  files: ReviewFile[]
  view: ReviewView
  pendingKey: string | null
  register: (path: string, node: HTMLElement | null) => void
  onDecide: (path: string, decision: 'approve' | 'reject', hunkIds?: string[]) => void
}

export function DiffList({ files, view, pendingKey, register, onDecide }: DiffListProps) {
  const [paint, setPaint] = useState<Record<string, Sides>>({})

  useEffect(() => {
    let cancelled = false
    void Promise.all(
      files.map(async (file) => {
        const sides = await paintSides(file.path, file.baseline, file.current)
        return [file.path, sides] as const
      })
    ).then((entries) => {
      if (!cancelled) {
        setPaint(Object.fromEntries(entries))
      }
    })
    return () => {
      cancelled = true
    }
  }, [files])

  return (
    <div className={styles.list}>
      {files.map((file) => {
        const visible = linesForView(file.lines, view)
        const note = reviewNote(file.status, view, visible)
        const chunks = chunksFor(visible, file.hunks)
        const filePending = pendingKey === file.path
        return (
          <section
            key={file.path}
            className={styles.file}
            ref={(node) => register(file.path, node)}
          >
            <header className={styles.header}>
              <span className={styles.identity}>
                <span className={styles.path}>{file.path}</span>
                <span className={styles.add}>+{file.additions}</span>
                <span className={styles.del}>-{file.deletions}</span>
              </span>
              <DecisionButtons
                disabled={pendingKey !== null}
                busy={filePending}
                onReject={() => onDecide(file.path, 'reject')}
                onApprove={() => onDecide(file.path, 'approve')}
              />
            </header>
            {note ? (
              <p className={styles.note}>{note}</p>
            ) : (
              <div className={styles.viewport}>
                {chunks.map((chunk, chunkIndex) => (
                  <Fragment key={`${file.path}-${chunkIndex}`}>
                    {chunkIndex > 0 ? <div className={styles.gap}>···</div> : null}
                    <div className={styles.chunk}>
                      {file.status !== 'added' && chunk.hunkIds.length > 0 ? (
                        <div
                          className={styles.chunkActions}
                          style={{ '--change-line': firstChangeLine(chunk.lines) }}
                        >
                          <DecisionButtons
                            disabled={pendingKey !== null}
                            busy={chunk.hunkIds.some((id) => pendingKey === `${file.path}:${id}`)}
                            onReject={() => onDecide(file.path, 'reject', chunk.hunkIds)}
                            onApprove={() => onDecide(file.path, 'approve', chunk.hunkIds)}
                          />
                        </div>
                      ) : null}
                      <div className={styles.hunk}>
                        <div className={styles.sheet}>
                          {chunk.lines.map((line, index) => (
                            <DiffLine
                              key={`${file.path}-${chunkIndex}-${index}`}
                              line={line}
                              tokens={tokensFor(line, paint[file.path])}
                            />
                          ))}
                        </div>
                      </div>
                    </div>
                  </Fragment>
                ))}
              </div>
            )}
          </section>
        )
      })}
    </div>
  )
}

function DecisionButtons({
  disabled,
  busy,
  onReject,
  onApprove
}: {
  disabled: boolean
  busy: boolean
  onReject: () => void
  onApprove: () => void
}) {
  return (
    <span className={styles.actions}>
      <button type="button" className={styles.reject} disabled={disabled} onClick={onReject}>
        Reject
      </button>
      <button type="button" className={styles.approve} disabled={disabled} onClick={onApprove}>
        {busy ? '…' : 'Approve'}
      </button>
    </span>
  )
}

function firstChangeLine(lines: ReviewLine[]): number {
  const index = lines.findIndex((line) => line.kind === 'insert' || line.kind === 'delete')
  return index < 0 ? 0 : index
}

function tokensFor(line: ReviewLine, sides: Sides | undefined): PaintedToken[] {
  if (line.kind === 'gap' || !sides) {
    return [{ text: line.text }]
  }
  const source = line.kind === 'delete' ? sides.baseline : sides.current
  const number = line.kind === 'delete' ? line.old_line : line.new_line
  if (number == null) {
    return [{ text: line.text }]
  }
  return source[number - 1] ?? [{ text: line.text }]
}

function DiffLine({ line, tokens }: { line: ReviewLine; tokens: PaintedToken[] }) {
  if (line.kind === 'gap') {
    return <div className={styles.gap}>···</div>
  }
  const mark = line.kind === 'delete' ? '−' : line.kind === 'insert' ? '+' : ' '
  const empty = tokens.every((token) => token.text === '')
  return (
    <div className={styles.line} data-kind={line.kind}>
      <span className={styles.num}>{line.old_line ?? ''}</span>
      <span className={styles.num}>{line.new_line ?? ''}</span>
      <span className={styles.mark}>{mark}</span>
      <span className={styles.code}>
        {empty
          ? '\u00a0'
          : tokens.map((token, index) => (
              <span key={index} style={token.color ? { color: token.color } : undefined}>
                {token.text}
              </span>
            ))}
      </span>
    </div>
  )
}
