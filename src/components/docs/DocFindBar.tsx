import { CaseSensitive, ChevronDown, ChevronUp, X } from 'lucide-react'
import type { KeyboardEvent, RefObject } from 'react'

import styles from './DocFindBar.module.css'

type DocFindBarProps = {
  query: string
  onQuery: (value: string) => void
  /** Total matches in the open document. */
  count: number
  /** Zero-based index of the active match. */
  current: number
  caseSensitive: boolean
  onCaseSensitive: (on: boolean) => void
  onNext: () => void
  onPrevious: () => void
  onClose: () => void
  inputRef: RefObject<HTMLInputElement | null>
}

export function DocFindBar({
  query,
  onQuery,
  count,
  current,
  caseSensitive,
  onCaseSensitive,
  onNext,
  onPrevious,
  onClose,
  inputRef
}: DocFindBarProps) {
  const empty = count === 0
  const status = query === '' ? '' : empty ? 'No results' : `${current + 1} of ${count}`

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Enter') {
      event.preventDefault()
      if (event.shiftKey) {
        onPrevious()
      } else {
        onNext()
      }
    }
  }

  return (
    <div className={styles.bar}>
      <input
        ref={inputRef}
        className={styles.input}
        type="text"
        value={query}
        placeholder="Find in document…"
        aria-label="Find in document"
        spellCheck={false}
        onChange={(event) => onQuery(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <span className={styles.count} role="status">
        {status}
      </span>
      <button
        type="button"
        className={styles.button}
        aria-label="Previous match"
        disabled={empty}
        onClick={onPrevious}
      >
        <ChevronUp size={14} strokeWidth={1.6} aria-hidden />
      </button>
      <button
        type="button"
        className={styles.button}
        aria-label="Next match"
        disabled={empty}
        onClick={onNext}
      >
        <ChevronDown size={14} strokeWidth={1.6} aria-hidden />
      </button>
      <button
        type="button"
        className={styles.button}
        aria-label="Match case"
        aria-pressed={caseSensitive}
        onClick={() => onCaseSensitive(!caseSensitive)}
      >
        <CaseSensitive size={15} strokeWidth={1.6} aria-hidden />
      </button>
      <button type="button" className={styles.button} aria-label="Close find" onClick={onClose}>
        <X size={14} strokeWidth={1.6} aria-hidden />
      </button>
    </div>
  )
}
