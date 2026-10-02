import { greetingLabel } from './greeting'
import styles from './EmptyGreeting.module.css'

export function EmptyGreeting() {
  return <h1 className={styles.greeting}>{greetingLabel(new Date())}</h1>
}
