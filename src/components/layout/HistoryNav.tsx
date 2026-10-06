import { ChevronLeft, ChevronRight } from 'lucide-react'
import { useLocation, useNavigate } from 'react-router-dom'

import { canGoForward, historyIndex } from '../../app/mouseHistory'
import styles from './HistoryNav.module.css'

/** Back and forward for the same history the side mouse buttons walk. */
export function HistoryNav() {
  const navigate = useNavigate()
  useLocation()
  const index = historyIndex()

  return (
    <div className={styles.nav}>
      <button
        type="button"
        className={styles.button}
        aria-label="Back"
        title="Back"
        disabled={index <= 0}
        onClick={() => navigate(-1)}
      >
        <ChevronLeft />
      </button>
      <button
        type="button"
        className={styles.button}
        aria-label="Forward"
        title="Forward"
        disabled={!canGoForward(index)}
        onClick={() => navigate(1)}
      >
        <ChevronRight />
      </button>
    </div>
  )
}
