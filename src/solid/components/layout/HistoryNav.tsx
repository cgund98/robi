/** @jsxImportSource solid-js */
import { useLocation, useNavigate } from '@solidjs/router'
import { ChevronLeft, ChevronRight } from '../../ui/icons'

import styles from '../../../components/layout/HistoryNav.module.css'
import { canSolidGoForward, solidHistoryIndex } from '../../app/useFrameChrome'

/** Back and forward for the same history the side mouse buttons walk. */
export function HistoryNav() {
  const navigate = useNavigate()
  const location = useLocation()
  const index = () => {
    location.key
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
