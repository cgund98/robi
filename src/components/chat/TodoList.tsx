import type { PlanTodo } from './toolCallView'
import type { FinishedTodo } from './todoProgress'
import styles from './TodoList.module.css'

const STATUS_LABEL = {
  pending: 'Pending',
  in_progress: 'In progress',
  completed: 'Completed',
  canceled: 'Canceled'
} as const

export function FinishedTodos({ items }: { items: FinishedTodo[] }) {
  if (items.length === 0) {
    return null
  }
  return (
    <ul className={styles.done}>
      {items.map((item) => (
        <li key={item.id} className={styles.doneItem} data-status={item.status}>
          <DoneMark status={item.status} />
          <span className={styles.text}>
            <span className={styles.sr}>{STATUS_LABEL[item.status]}. </span>
            {item.content}
          </span>
        </li>
      ))}
    </ul>
  )
}

export function RemainingTodos({ items }: { items: PlanTodo[] }) {
  if (items.length === 0) {
    return null
  }
  return (
    <li className={styles.remaining}>
      <p className={styles.label}>Remaining</p>
      <ol className={styles.list} aria-label="Remaining tasks">
        {items.map((item) => (
          <li key={item.id} className={styles.item} data-status={item.status}>
            <span className={styles.mark} aria-hidden />
            <span className={styles.text} title={item.content}>
              <span className={styles.sr}>{STATUS_LABEL[item.status]}. </span>
              {item.content}
            </span>
          </li>
        ))}
      </ol>
    </li>
  )
}

function DoneMark({ status }: { status: FinishedTodo['status'] }) {
  if (status === 'canceled') {
    return <span className={styles.mark} data-status="canceled" aria-hidden />
  }
  return (
    <svg className={styles.check} viewBox="0 0 16 16" aria-hidden>
      <circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" strokeWidth="1.4" />
      <path
        d="M5.2 8.2 7.1 10.1 10.8 6.2"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}
