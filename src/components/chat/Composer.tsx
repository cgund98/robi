import { useState } from 'react'

import styles from './Composer.module.css'

export function Composer() {
  const [draft, setDraft] = useState('')

  return (
    <div className={styles.composer}>
      <div className={styles.column}>
        <div className={styles.field}>
          <textarea
            className={styles.input}
            rows={1}
            placeholder="Describe a task or ask a question"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            aria-label="Message"
          />
          <button type="button" className={styles.send} disabled={!draft.trim()} aria-label="Send">
            ⏎
          </button>
        </div>

        <div className={styles.toolbar}>
          <div className={styles.cluster}>
            <button type="button" className={styles.control}>
              Model ▾
            </button>
            <button type="button" className={styles.control}>
              Extra high ▾
            </button>
            <span className={styles.status} aria-hidden title="Idle" />
          </div>
        </div>
      </div>
    </div>
  )
}
