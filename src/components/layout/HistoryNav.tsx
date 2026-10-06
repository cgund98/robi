/** @jsxImportSource solid-js */
import { useLocation, useNavigate } from '@solidjs/router'
import { ChevronLeft, ChevronRight } from '../ui/icons'

import styles from './HistoryNav.module.css'
import { canSolidGoForward, solidHistoryIndex } from '../../app/useFrameChrome'

/** Back and forward for the same history the side mouse buttons walk. */
export function HistoryNav() {
  const navigate = useNavigate()
  const location = useLocation()
  const index = () => {
    // A document change only updates the query, and this router's `key` never
    // changes, so the buttons have to read the search to notice a new entry.
    location.pathname
    location.search
    return solidHistoryIndex()
  }

  return (
    <div class={styles.nav}>
      <button
        type="button"
        class={styles.button}
        aria-label="Back"
        title="Back"
        disabled={index() <= 0}
        onClick={() => navigate(-1)}
      >
        <ChevronLeft />
      </button>
      <button
        type="button"
        class={styles.button}
        aria-label="Forward"
        title="Forward"
        disabled={!canSolidGoForward(index())}
        onClick={() => navigate(1)}
      >
        <ChevronRight />
      </button>
    </div>
  )
}
