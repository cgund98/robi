/** @jsxImportSource solid-js */
import { For, Show } from 'solid-js'

import type { MockFileEdit } from '../../../mock/chat'
import styles from '../../../components/chat/FileEditSummary.module.css'

export function FileEditSummary(props: {
  filesEdited: number
  additions: number
  deletions: number
  files: MockFileEdit[]
}) {
  return (
    <div class={styles.widget}>
      <div class={styles.header}>
        <div class={styles.summary}>
          <span>
            {props.filesEdited} file{props.filesEdited === 1 ? '' : 's'} edited
          </span>
          <span class={styles.counts}>
            <span class={styles.add}>+{props.additions}</span>
            <span class={styles.del}>-{props.deletions}</span>
          </span>
        </div>
        <button type="button" class={styles.review} disabled title="Review window arrives in M6">
          Review ↗
        </button>
      </div>
      <ul class={styles.list}>
        <For each={props.files}>
          {(file) => (
            <li>
              <button type="button" class={styles.row}>
                <span class={styles.path}>{file.path}</span>
                <span class={styles.rowCounts}>
                  <span class={styles.add}>+{file.additions}</span>
                  <span class={styles.del}>-{file.deletions}</span>
                </span>
                <Show when={file.unread}>
                  <span class={styles.dot} aria-label="Unread" />
                </Show>
                <span class={styles.chevron} aria-hidden="true">
                  ▾
                </span>
              </button>
            </li>
          )}
        </For>
      </ul>
    </div>
  )
}
