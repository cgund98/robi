/** @jsxImportSource solid-js */
import { useNavigate } from '@solidjs/router'
import { BookOpen, ChatBubble } from '../../ui/icons'

import { chatRoute } from '../../../app/chatRoute'
import styles from '../../../components/layout/ViewToggle.module.css'
import { chat } from '../../state/host'

/** Chat and documentation, as a pair of icons in the window bar. */
export function ViewToggle(props: { docsOpen: boolean }) {
  const navigate = useNavigate()

  return (
    <div class={styles.toggle} role="group" aria-label="View">
      <button
        type="button"
        class={`${styles.button} ${styles.buttonAsk} ${props.docsOpen ? '' : styles.buttonOn}`}
        aria-pressed={!props.docsOpen}
        title="Chat"
        aria-label="Chat"
        onClick={() => {
          if (props.docsOpen) navigate(chatRoute(chat.activeSessionId, chat.draftSelected))
        }}
      >
        <ChatBubble />
      </button>
      <button
        type="button"
        class={`${styles.button} ${styles.buttonPlan} ${props.docsOpen ? styles.buttonOn : ''}`}
        aria-pressed={props.docsOpen}
        title="Documentation"
        aria-label="Documentation"
        onClick={() => {
          if (!props.docsOpen) navigate('/docs')
        }}
      >
        <BookOpen />
      </button>
    </div>
  )
}
