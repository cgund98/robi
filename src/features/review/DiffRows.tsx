/** @jsxImportSource solid-js */
import { For } from 'solid-js'

import type { ReviewLine } from '../../api/review'
import type { PaintedToken } from './highlight'
import styles from './DiffList.module.css'

export function DiffRows(props: {
  lines: ReviewLine[]
  tokensForLine: (line: ReviewLine) => PaintedToken[]
}) {
  return (
    <div class={styles.sheet}>
      <div class={styles.gutter} aria-hidden="true">
        <For each={props.lines}>
          {(line) => (
            <div class={styles.gutterLine} data-kind={line.kind}>
              <span class={styles.num}>{line.old_line ?? ''}</span>
              <span class={styles.num}>{line.new_line ?? ''}</span>
              <span class={styles.mark}>
                {line.kind === 'delete' ? '−' : line.kind === 'insert' ? '+' : ' '}
              </span>
            </div>
          )}
        </For>
      </div>
      <div class={styles.codeCol}>
        <For each={props.lines}>
          {(line) => {
            const tokens = () => props.tokensForLine(line)
            const empty = () => tokens().every((token) => token.text === '')
            return (
              <div class={styles.codeLine} data-kind={line.kind}>
                {empty()
                  ? '\u00a0'
                  : tokens().map((token) => (
                      <span style={token.color ? { color: token.color } : undefined}>
                        {token.text}
                      </span>
                    ))}
              </div>
            )
          }}
        </For>
      </div>
    </div>
  )
}
