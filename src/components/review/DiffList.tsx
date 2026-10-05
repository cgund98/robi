import { Fragment, useEffect, useState } from 'react'

import type { ReviewFile, ReviewLine } from '../../api/review'
import { chunksFor, linesForView, reviewNote, type ReviewView } from './diffView'
import { DiffRows } from './DiffRows'
import { FileViewDialog } from './FileViewDialog'
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
  onDecide: (path: string, decision: 'approve' | 'reject', hunkIds?: string[]) => void
}

export function DiffList({ files, view, pendingKey, onDecide }: DiffListProps) {
  const [paint, setPaint] = useState<Record<string, Sides>>({})
  const [openPath, setOpenPath] = useState<string | null>(null)
  const [hoverChunk, setHoverChunk] = useState<string | null>(null)

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

  const openFile = files.find((file) => file.path === openPath) ?? null

  return (
    <div className={styles.list}>
      <FileViewDialog
        file={openFile}
        baseline={paintedSide(openFile, openFile ? paint[openFile.path] : undefined, 'baseline')}
        current={paintedSide(openFile, openFile ? paint[openFile.path] : undefined, 'current')}
        onClose={() => setOpenPath(null)}
      />
      {files.map((file) => {
        const visible = linesForView(file.lines, view)
        const note = reviewNote(file.status, view, visible)
        const chunks = chunksFor(visible, file.hunks)
        const filePending = pendingKey === file.path
        return (
          <section key={file.path} className={styles.file}>
            <header className={styles.header}>
              <span className={styles.identity}>
                <button
                  type="button"
                  className={styles.path}
                  onClick={() => setOpenPath(file.path)}
                >
                  {file.path}
                </button>
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
              <div className={styles.frame}>
                <div className={styles.viewport}>
                  <div className={styles.wide}>
                    {chunks.map((chunk, chunkIndex) => (
                      <Fragment key={`${file.path}-${chunkIndex}`}>
                        {chunkIndex > 0 ? <div className={styles.gap}>···</div> : null}
                        <div
                          className={styles.chunk}
                          onMouseEnter={() => setHoverChunk(`${file.path}:${chunkIndex}`)}
                          onMouseLeave={() =>
                            setHoverChunk((current) =>
                              current === `${file.path}:${chunkIndex}` ? null : current
                            )
                          }
                        >
                          <DiffRows
                            lines={chunk.lines}
                            tokensForLine={(line) => tokensFor(line, paint[file.path])}
                          />
                        </div>
                      </Fragment>
                    ))}
                  </div>
                </div>
                <div className={styles.overlays}>
                  {chunks.map((chunk, chunkIndex) =>
                    file.status !== 'added' && chunk.hunkIds.length > 0 ? (
                      <div
                        key={`${file.path}-${chunkIndex}`}
                        className={styles.chunkActions}
                        data-open={hoverChunk === `${file.path}:${chunkIndex}` ? 'true' : undefined}
                        style={{
                          top: `calc(${chunkStart(chunks, chunkIndex) + firstChangeLine(chunk.lines)} * var(--review-line))`
                        }}
                        onMouseEnter={() => setHoverChunk(`${file.path}:${chunkIndex}`)}
                        onMouseLeave={() =>
                          setHoverChunk((current) =>
                            current === `${file.path}:${chunkIndex}` ? null : current
                          )
                        }
                      >
                        <DecisionButtons
                          disabled={pendingKey !== null}
                          busy={chunk.hunkIds.some((id) => pendingKey === `${file.path}:${id}`)}
                          onReject={() => onDecide(file.path, 'reject', chunk.hunkIds)}
                          onApprove={() => onDecide(file.path, 'approve', chunk.hunkIds)}
                        />
                      </div>
                    ) : null
                  )}
                </div>
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

function chunkStart(chunks: { lines: ReviewLine[] }[], index: number): number {
  let count = 0
  for (let i = 0; i < index; i++) {
    count += chunks[i].lines.length + 1
  }
  return count
}

function paintedSide(
  file: ReviewFile | null,
  sides: Sides | undefined,
  which: 'baseline' | 'current'
): PaintedToken[][] {
  if (!file) {
    return []
  }
  const painted = which === 'baseline' ? sides?.baseline : sides?.current
  return painted ?? plainLines(which === 'baseline' ? file.baseline : file.current)
}

function plainLines(text: string): PaintedToken[][] {
  if (text === '') {
    return []
  }
  const lines = text.split(/\r?\n/)
  if (lines[lines.length - 1] === '') {
    lines.pop()
  }
  return lines.map((line) => [{ text: line }])
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
