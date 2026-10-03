import { AssistantMarkdown } from './AssistantMarkdown'
import type { PlanTodo, PlanView } from './toolCallView'
import styles from './PlanPage.module.css'

const STATUS_LABEL: Record<PlanTodo['status'], string> = {
  pending: 'Pending',
  in_progress: 'In progress',
  completed: 'Completed',
  canceled: 'Canceled'
}

type PlanPageProps = {
  plan: PlanView
  buildDisabled: boolean
  onBack: () => void
  onBuild: () => void
}

export function PlanPage({ plan, buildDisabled, onBack, onBuild }: PlanPageProps) {
  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <button type="button" className={styles.back} onClick={onBack}>
          ← Back
        </button>
        <h1 className={styles.title}>{plan.title}</h1>
        <button type="button" className={styles.build} disabled={buildDisabled} onClick={onBuild}>
          Build
        </button>
      </header>
      <div className={styles.scroll}>
        <div className={styles.column}>
          {plan.todos.length > 0 ? (
            <ol className={styles.todos}>
              {plan.todos.map((todo) => (
                <li key={todo.id} className={styles.todo} data-status={todo.status}>
                  <span className={styles.mark} aria-hidden />
                  <span className={styles.todoText}>
                    <span className={styles.sr}>{STATUS_LABEL[todo.status]}. </span>
                    {todo.content}
                  </span>
                </li>
              ))}
            </ol>
          ) : null}
          <AssistantMarkdown text={plan.body} />
        </div>
      </div>
    </div>
  )
}
