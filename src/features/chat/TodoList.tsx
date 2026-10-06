/** @jsxImportSource solid-js */
import { CheckCircle } from '../../components/ui/icons'
import { For, Show } from 'solid-js'

import type { PlanTodo } from './toolCallView'
import type { FinishedTodo } from './todoProgress'
import styles from './TodoList.module.css'

const STATUS_LABEL = {
  pending: 'Pending',
  in_progress: 'In progress',
  completed: 'Completed',
  canceled: 'Canceled'
} as const

export function FinishedTodos(props: { items: FinishedTodo[] }) {
  return (
    <Show when={props.items.length > 0}>
      <ul class={styles.done}>
        <For each={props.items}>
          {(item) => (
            <li class={styles.doneItem} data-status={item.status}>
              <DoneMark status={item.status} />
              <span class={styles.text}>
                <span class={styles.sr}>{STATUS_LABEL[item.status]}. </span>
                {item.content}
              </span>
            </li>
          )}
        </For>
      </ul>
    </Show>
  )
}

export function RemainingTodos(props: { items: PlanTodo[] }) {
  return (
    <Show when={props.items.length > 0}>
      <li class={styles.remaining}>
        <p class={styles.label}>Remaining</p>
        <ol class={styles.list} aria-label="Remaining tasks">
          <For each={props.items}>
            {(item) => (
              <li class={styles.item} data-status={item.status}>
                <span class={styles.mark} aria-hidden="true" />
                <span class={styles.text} title={item.content}>
                  <span class={styles.sr}>{STATUS_LABEL[item.status]}. </span>
                  {item.content}
                </span>
              </li>
            )}
          </For>
        </ol>
      </li>
    </Show>
  )
}

function DoneMark(props: { status: FinishedTodo['status'] }) {
  return (
    <Show
      when={props.status !== 'canceled'}
      fallback={<span class={styles.mark} data-status="canceled" aria-hidden="true" />}
    >
      <CheckCircle class={styles.check} size={14} aria-hidden="true" />
    </Show>
  )
}
