import { FileText } from 'lucide-react'

import styles from './AttachmentChip.module.css'

type AttachmentChipProps = {
  /** Display name, e.g. `error.rs`. */
  name: string
  /** Workspace-relative path when the file is inside the workspace. */
  path?: string | null
  /** 1-based first line of the attached slice, when it was a range. */
  startLine?: number | null
  /** 1-based last line of the slice, inclusive, when it was a range. */
  endLine?: number | null
  /** When set, a remove control is shown (the composer's copy). */
  onRemove?: () => void
}

/** `filename (1-10)`, the same pill in the composer, the transcript, and the echo. */
export function AttachmentChip({ name, path, startLine, endLine, onRemove }: AttachmentChipProps) {
  const range =
    startLine != null && endLine != null
      ? startLine === endLine
        ? ` (${startLine})`
        : ` (${startLine}-${endLine})`
      : ''
  // A file with no workspace path came from outside the workspace; the tooltip
  // says so, since the model is told the same and cannot re-fetch it.
  const origin = path ?? 'outside the workspace'
  return (
    <div className={styles.chip} title={`${name}${range} — ${origin}`}>
      <FileText size={14} strokeWidth={1.75} className={styles.icon} aria-hidden />
      <span className={styles.name}>{name}</span>
      {range ? <span className={styles.range}>{range}</span> : null}
      {onRemove ? (
        <button
          type="button"
          className={styles.remove}
          aria-label={`Remove ${name}`}
          onClick={onRemove}
        >
          ×
        </button>
      ) : null}
    </div>
  )
}
