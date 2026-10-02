import type { ReactNode } from 'react'

import type { MockTranscriptItem } from '../../mock/chat'
import { FileEditSummary } from './FileEditSummary'
import styles from './Transcript.module.css'

type TranscriptProps = {
  items: MockTranscriptItem[]
}

function renderInlineCode(text: string): ReactNode[] {
  const parts = text.split(/(`[^`]+`)/g)
  return parts.map((part, index) => {
    if (part.startsWith('`') && part.endsWith('`') && part.length > 2) {
      return <code key={index}>{part.slice(1, -1)}</code>
    }
    return <span key={index}>{part}</span>
  })
}

export function Transcript({ items }: TranscriptProps) {
  return (
    <div className={styles.transcript}>
      <ul className={styles.list}>
        {items.map((item) => {
          switch (item.kind) {
            case 'user':
              return (
                <li key={item.id} className={styles.user}>
                  {item.text}
                </li>
              )
            case 'assistant':
              return (
                <li key={item.id} className={styles.assistant}>
                  {renderInlineCode(item.text)}
                </li>
              )
            case 'activity':
              return (
                <li key={item.id} className={styles.activity}>
                  <span className={styles.activityIcon} aria-hidden>
                    ●
                  </span>
                  {item.text}
                </li>
              )
            case 'file-edits':
              return (
                <li key={item.id}>
                  <FileEditSummary
                    filesEdited={item.filesEdited}
                    additions={item.additions}
                    deletions={item.deletions}
                    files={item.files}
                  />
                </li>
              )
          }
        })}
      </ul>
    </div>
  )
}
