/** @jsxImportSource solid-js */
import { greetingLabel } from '../../../components/chat/greeting'
import styles from '../../../components/chat/EmptyGreeting.module.css'

export function EmptyGreeting() {
  return <h1 class={styles.greeting}>{greetingLabel(new Date())}</h1>
}
