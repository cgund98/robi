import { BookOpen, MessageSquare } from 'lucide-react'
import { useNavigate } from 'react-router-dom'

import { chatRoute } from '../../app/chatRoute'
import { useChatStore } from '../../state/chatStore'
import styles from './ViewToggle.module.css'

/** Chat and documentation, as a pair of icons in the window bar. */
export function ViewToggle({ docsOpen }: { docsOpen: boolean }) {
  const navigate = useNavigate()
  const activeSessionId = useChatStore((state) => state.activeSessionId)
  const draftSelected = useChatStore((state) => state.draftSelected)

  return (
    <div className={styles.toggle} role="group" aria-label="View">
      <button
        type="button"
        className={`${styles.button} ${styles.buttonAsk} ${docsOpen ? '' : styles.buttonOn}`}
        aria-pressed={!docsOpen}
        title="Chat"
        aria-label="Chat"
        onClick={() => {
          if (docsOpen) navigate(chatRoute(activeSessionId, draftSelected))
        }}
      >
        <MessageSquare />
      </button>
      <button
        type="button"
        className={`${styles.button} ${styles.buttonPlan} ${docsOpen ? styles.buttonOn : ''}`}
        aria-pressed={docsOpen}
        title="Documentation"
        aria-label="Documentation"
        onClick={() => {
          if (!docsOpen) navigate('/docs')
        }}
      >
        <BookOpen />
      </button>
    </div>
  )
}
