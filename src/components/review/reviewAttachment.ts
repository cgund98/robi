import type { ReviewFile, ReviewHunk } from '../../api/review'
import { bytesToBase64, MAX_TEXT_FILE_BYTES, type FileAttachment } from '../chat/textAttachments'

/** The slice of the file an attachment carries, and its 1-based range. */
type Slice = {
  text: string
  /** Absent for a whole-file attach, which has no range. */
  startLine?: number
  endLine?: number
}

function basename(path: string): string {
  const cut = path.lastIndexOf('/')
  return cut < 0 ? path : path.slice(cut + 1)
}

/** Split a file body the same way the review lines are split. */
function splitLines(text: string): string[] {
  if (text === '') {
    return []
  }
  const lines = text.split(/\r?\n/)
  if (lines[lines.length - 1] === '') {
    lines.pop()
  }
  return lines
}

function build(file: ReviewFile, root: string | null, slice: Slice): FileAttachment {
  const bytes = new TextEncoder().encode(slice.text)
  if (bytes.length > MAX_TEXT_FILE_BYTES) {
    throw new Error(
      `${basename(file.path)} is larger than ${Math.round(MAX_TEXT_FILE_BYTES / 1024)} KB`
    )
  }
  return {
    name: basename(file.path),
    path: file.path,
    absolutePath: root ? `${root.replace(/\/+$/, '')}/${file.path}` : undefined,
    startLine: slice.startLine,
    endLine: slice.endLine,
    contentBase64: bytesToBase64(bytes),
    size: bytes.length
  }
}

/**
 * The whole reviewed file as one attachment, with no line range.
 *
 * The bytes are the file's current body. A file this session deleted has none
 * on disk, so the baseline stands in — that is what the user is rejecting.
 */
export function wholeFileAttachment(file: ReviewFile, root: string | null): FileAttachment {
  const text = file.status === 'deleted' ? file.baseline : file.current
  return build(file, root, { text })
}

/**
 * The current lines a rejected hunk covers, 1-based and inclusive.
 *
 * `new_start` and `new_count` describe the hunk on the current side. A hunk
 * that only deletes has no current lines: the deletion point is used instead,
 * and a file emptied by the deletion falls back to the baseline lines the hunk
 * removed.
 */
export function hunkSlice(
  file: ReviewFile,
  hunk: ReviewHunk
): Slice & { startLine: number; endLine: number } {
  const current = splitLines(file.current)
  if (hunk.new_count > 0) {
    const start = hunk.new_start + 1
    const end = hunk.new_start + hunk.new_count
    const slice = current.slice(hunk.new_start, hunk.new_start + hunk.new_count)
    return { text: slice.join('\n'), startLine: start, endLine: end }
  }
  if (current.length === 0) {
    const baseline = splitLines(file.baseline)
    const start = hunk.old_start + 1
    const end = Math.max(start, hunk.old_start + hunk.old_count)
    const slice = baseline.slice(hunk.old_start, hunk.old_start + hunk.old_count)
    return { text: slice.join('\n'), startLine: start, endLine: end }
  }
  // Keep the deletion point: the line where the removed lines were, or the last
  // line when the deletion ran to the end of the file.
  const index = Math.min(hunk.new_start, current.length - 1)
  const line = index + 1
  return { text: current[index] ?? '', startLine: line, endLine: line }
}

/**
 * The lines a rejected hunk covers, as one attachment.
 *
 * See [`hunkSlice`] for the range it uses.
 */
export function hunkAttachment(
  file: ReviewFile,
  hunk: ReviewHunk,
  root: string | null
): FileAttachment {
  return build(file, root, hunkSlice(file, hunk))
}

/**
 * The message sent after a reason is given, telling the model which change the
 * user rejected and what to change.
 */
export function rejectReasonInstruction(
  path: string,
  range: { start: number; end: number } | null,
  reason: string
): string {
  const where =
    range === null
      ? path
      : range.start === range.end
        ? `${path} (line ${range.start})`
        : `${path} (lines ${range.start}-${range.end})`
  return `I rejected the change to ${where}. Please iterate on the file using this feedback:\n\n${reason}`
}
