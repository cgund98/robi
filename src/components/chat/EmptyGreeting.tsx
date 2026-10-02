import { greetingLabel } from './greeting'
import styles from './EmptyGreeting.module.css'

export function EmptyGreeting() {
  return (
    <h1 className={styles.greeting}>
      <span className={styles.mark} aria-hidden>
        <svg width="28" height="28" viewBox="0 0 24 24" fill="currentColor">
          <rect x="11" y="1.5" width="2" height="21" rx="1" />
          <rect x="1.5" y="11" width="21" height="2" rx="1" />
          <rect x="11" y="1.5" width="2" height="21" rx="1" transform="rotate(45 12 12)" />
          <rect x="11" y="1.5" width="2" height="21" rx="1" transform="rotate(135 12 12)" />
        </svg>
      </span>
      {greetingLabel(new Date())}
    </h1>
  )
}
