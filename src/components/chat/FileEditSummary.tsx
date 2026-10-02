import type { MockFileEdit } from '../../mock/chat'
import styles from './FileEditSummary.module.css'

type FileEditSummaryProps = {
  filesEdited: number
  additions: number
  deletions: number
  files: MockFileEdit[]
}

export function FileEditSummary({
  filesEdited,
  additions,
  deletions,
  files
}: FileEditSummaryProps) {
  return (
    <div className={styles.widget}>
      <div className={styles.header}>
        <div className={styles.summary}>
          <span>
            {filesEdited} file{filesEdited === 1 ? '' : 's'} edited
          </span>
          <span className={styles.counts}>
            <span className={styles.add}>+{additions}</span>
            <span className={styles.del}>-{deletions}</span>
          </span>
        </div>
        {/* Review destination arrives with M6 — keep affordance visible but inert. */}
        <button
          type="button"
          className={styles.review}
          disabled
          title="Review window arrives in M6"
        >
          Review ↗
        </button>
      </div>

      <ul className={styles.list}>
        {files.map((file) => (
          <li key={file.path}>
            <button type="button" className={styles.row}>
              <span className={styles.path}>{file.path}</span>
              <span className={styles.rowCounts}>
                <span className={styles.add}>+{file.additions}</span>
                <span className={styles.del}>-{file.deletions}</span>
              </span>
              {file.unread ? <span className={styles.dot} aria-label="Unread" /> : null}
              <span className={styles.chevron} aria-hidden>
                ▾
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
