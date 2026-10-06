/** @jsxImportSource solid-js */
import { ChevronDown, ChevronUp, Language, XMark } from '../../ui/icons'

import styles from '../../../components/docs/DocFindBar.module.css'

export function DocFindBar(props: {
  query: string
  onQuery: (value: string) => void
  count: number
  current: number
  caseSensitive: boolean
  onCaseSensitive: (on: boolean) => void
  onNext: () => void
  onPrevious: () => void
  onClose: () => void
  input: (el: HTMLInputElement) => void
}) {
  const empty = () => props.count === 0
  const status = () =>
    props.query === '' ? '' : empty() ? 'No results' : `${props.current + 1} of ${props.count}`

  return (
    <div class={styles.bar}>
      <input
        ref={props.input}
        class={styles.input}
        type="text"
        value={props.query}
        placeholder="Find in document…"
        aria-label="Find in document"
        spellcheck={false}
        onInput={(event) => props.onQuery(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault()
            if (event.shiftKey) {
              props.onPrevious()
            } else {
              props.onNext()
            }
          }
        }}
      />
      <span class={styles.count} role="status">
        {status()}
      </span>
      <button
        type="button"
        class={styles.button}
        aria-label="Previous match"
        disabled={empty()}
        onClick={() => props.onPrevious()}
      >
        <ChevronUp size={14} aria-hidden="true" />
      </button>
      <button
        type="button"
        class={styles.button}
        aria-label="Next match"
        disabled={empty()}
        onClick={() => props.onNext()}
      >
        <ChevronDown size={14} aria-hidden="true" />
      </button>
      <button
        type="button"
        class={styles.button}
        aria-label="Match case"
        aria-pressed={props.caseSensitive}
        onClick={() => props.onCaseSensitive(!props.caseSensitive)}
      >
        <Language size={15} aria-hidden="true" />
      </button>
      <button
        type="button"
        class={styles.button}
        aria-label="Close find"
        onClick={() => props.onClose()}
      >
        <XMark size={14} aria-hidden="true" />
      </button>
    </div>
  )
}
