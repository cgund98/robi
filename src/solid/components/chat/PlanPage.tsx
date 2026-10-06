/** @jsxImportSource solid-js */
import { For, Show } from 'solid-js'

import { AssistantMarkdown } from './AssistantMarkdown'
import type { PlanTodo, PlanView } from '../../../components/chat/toolCallView'
import styles from '../../../components/chat/PlanPage.module.css'

const STATUS_LABEL: Record<PlanTodo['status'], string> = {
  pending: 'Pending',
  in_progress: 'In progress',
  completed: 'Completed',
  canceled: 'Canceled'
}

export function PlanPage(props: {
  plan: PlanView
  buildDisabled: boolean
  onBack: () => void
  onBuild: () => void
}) {
  return (
    <div class={styles.page}>
      <header class={styles.header}>
        <button type="button" class={styles.back} onClick={() => props.onBack()}>
          ← Back
        </button>
        <h1 class={styles.title}>{props.plan.title}</h1>
        <button
          type="button"
          class={styles.build}
          disabled={props.buildDisabled}
          onClick={() => props.onBuild()}
        >
          Build
        </button>
      </header>
      <div class={styles.scroll}>
        <div class={styles.column}>
          <Show when={props.plan.todos.length > 0}>
            <ol class={styles.todos}>
              <For each={props.plan.todos}>
                {(todo) => (
                  <li class={styles.todo} data-status={todo.status}>
                    <span class={styles.mark} aria-hidden="true" />
                    <span class={styles.todoText}>
                      <span class={styles.sr}>{STATUS_LABEL[todo.status]}. </span>
                      {todo.content}
                    </span>
                  </li>
                )}
              </For>
            </ol>
          </Show>
          <AssistantMarkdown text={props.plan.body} />
        </div>
      </div>
    </div>
  )
}
