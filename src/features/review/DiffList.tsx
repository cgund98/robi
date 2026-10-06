/** @jsxImportSource solid-js */
import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { ChevronDown } from '../../components/ui/icons'
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'

import type { ReviewFile, ReviewLine } from '../../api/review'
import { chunksFor, linesForView, reviewNote, type ReviewView } from './diffView'
import { paintSides, type PaintedToken } from './highlight'
import styles from './DiffList.module.css'
import { DiffRows } from './DiffRows'
import { FileViewDialog } from './FileViewDialog'

type Sides = {
  baseline: PaintedToken[][]
  current: PaintedToken[][]
}

export function DiffList(props: {
  files: ReviewFile[]
  view: ReviewView
  pendingKey: string | null
  onDecide: (path: string, decision: 'approve' | 'reject', hunkIds?: string[]) => void
  onRejectWithReason: (path: string, hunkIds?: string[]) => void
}) {
  const [paint, setPaint] = createSignal<Record<string, Sides>>({})
  const [openPath, setOpenPath] = createSignal<string | null>(null)
  const [hoverChunk, setHoverChunk] = createSignal<string | null>(null)
  const openFile = () => props.files.find((file) => file.path === openPath()) ?? null

  createEffect(() => {
    const files = props.files
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
    onCleanup(() => {
      cancelled = true
    })
  })

  return (
    <div class={styles.list}>
      <FileViewDialog
        file={openFile()}
        baseline={paintedSide(
          openFile(),
          openFile() ? paint()[openFile()!.path] : undefined,
          'baseline'
        )}
        current={paintedSide(
          openFile(),
          openFile() ? paint()[openFile()!.path] : undefined,
          'current'
        )}
        onClose={() => setOpenPath(null)}
      />
      <For each={props.files}>
        {(file) => {
          const visible = () => linesForView(file.lines, props.view)
          const note = () => reviewNote(file.status, props.view, visible())
          const chunks = () => chunksFor(visible(), file.hunks)
          const filePending = () => props.pendingKey === file.path
          return (
            <section class={styles.file}>
              <header class={styles.header}>
                <span class={styles.identity}>
                  <button type="button" class={styles.path} onClick={() => setOpenPath(file.path)}>
                    {file.path}
                  </button>
                  <span class={styles.add}>+{file.additions}</span>
                  <span class={styles.del}>-{file.deletions}</span>
                </span>
                <DecisionButtons
                  disabled={props.pendingKey !== null}
                  busy={filePending()}
                  onReject={() => props.onDecide(file.path, 'reject')}
                  onRejectWithReason={() => props.onRejectWithReason(file.path)}
                  onApprove={() => props.onDecide(file.path, 'approve')}
                />
              </header>
              <Show when={!note()} fallback={<p class={styles.note}>{note()}</p>}>
                <div class={styles.frame}>
                  <div class={styles.viewport}>
                    <div class={styles.wide}>
                      <For each={chunks()}>
                        {(chunk, chunkIndex) => (
                          <>
                            <Show when={chunkIndex() > 0}>
                              <div class={styles.gap}>···</div>
                            </Show>
                            <div
                              class={styles.chunk}
                              onMouseEnter={() => setHoverChunk(`${file.path}:${chunkIndex()}`)}
                              onMouseLeave={() =>
                                setHoverChunk((current) =>
                                  current === `${file.path}:${chunkIndex()}` ? null : current
                                )
                              }
                            >
                              <DiffRows
                                lines={chunk.lines}
                                tokensForLine={(line) => tokensFor(line, paint()[file.path])}
                              />
                            </div>
                          </>
                        )}
                      </For>
                    </div>
                  </div>
                  <div class={styles.overlays}>
                    <For each={chunks()}>
                      {(chunk, chunkIndex) => (
                        <Show when={file.status !== 'added' && chunk.hunks.length > 0}>
                          <For each={chunk.hunks}>
                            {(hunk) => (
                              <div
                                class={styles.chunkActions}
                                data-open={
                                  hoverChunk() === `${file.path}:${chunkIndex()}`
                                    ? 'true'
                                    : undefined
                                }
                                style={{
                                  top: `calc(${chunkStart(chunks(), chunkIndex()) + hunk.firstChange} * var(--review-line))`
                                }}
                                onMouseEnter={() => setHoverChunk(`${file.path}:${chunkIndex()}`)}
                                onMouseLeave={() =>
                                  setHoverChunk((current) =>
                                    current === `${file.path}:${chunkIndex()}` ? null : current
                                  )
                                }
                              >
                                <DecisionButtons
                                  disabled={props.pendingKey !== null}
                                  busy={props.pendingKey === `${file.path}:${hunk.id}`}
                                  onReject={() => props.onDecide(file.path, 'reject', [hunk.id])}
                                  onRejectWithReason={() =>
                                    props.onRejectWithReason(file.path, [hunk.id])
                                  }
                                  onApprove={() => props.onDecide(file.path, 'approve', [hunk.id])}
                                />
                              </div>
                            )}
                          </For>
                        </Show>
                      )}
                    </For>
                  </div>
                </div>
              </Show>
            </section>
          )
        }}
      </For>
    </div>
  )
}

function DecisionButtons(props: {
  disabled: boolean
  busy: boolean
  onReject: () => void
  onRejectWithReason: () => void
  onApprove: () => void
}) {
  return (
    <span class={styles.actions}>
      <span class={styles.rejectSplit}>
        <button
          type="button"
          class={styles.reject}
          disabled={props.disabled}
          onClick={() => props.onReject()}
        >
          Reject
        </button>
        <DropdownMenu>
          <DropdownMenu.Trigger
            class={styles.rejectChevron}
            aria-label="More reject options"
            disabled={props.disabled}
          >
            <ChevronDown size={12} />
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content class={styles.menu}>
              <DropdownMenu.Item
                class={styles.menuItem}
                onSelect={() => props.onRejectWithReason()}
              >
                Reject with reason
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu>
      </span>
      <button
        type="button"
        class={styles.approve}
        disabled={props.disabled}
        onClick={() => props.onApprove()}
      >
        {props.busy ? '…' : 'Approve'}
      </button>
    </span>
  )
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
