import { bytesToBase64, MAX_TEXT_FILE_BYTES, type FileAttachment } from '../chat/textAttachments'

/** The document the slice is taken from, named by the viewer. */
export type DocumentSlice = {
  /** The whole document's raw text, as the viewer loaded it. */
  content: string
  /** The workspace-relative markdown path, e.g. `docs/src/design/shell/chat-ui.md`. */
  path: string
  /** The workspace root, so the server can classify the file. Null drops the path. */
  root?: string | null
  /** 1-based first raw source line of the block. */
  startLine: number
  /** 1-based last raw source line, inclusive. */
  endLine: number
}

function basename(path: string): string {
  const cut = path.lastIndexOf('/')
  return cut < 0 ? path : path.slice(cut + 1)
}

/**
 * One rendered block of the open document as a ready file attachment.
 *
 * The slice is the block's **raw** markdown lines, not the rendered text: the
 * viewer attaches what the file says, so the model reads the source (a table's
 * pipe syntax, a fence's backticks) and can widen the read with `read_file`.
 * `start_line` / `end_line` are 1-based and inclusive, matching `read_file`.
 *
 * A workspace root is turned into an `absolutePath` so the server's
 * `classify_path` stores the workspace-relative path and the model can re-read
 * it. Without a root the attachment still builds, but the server treats it as
 * outside the workspace — the safe default.
 *
 * Throws when the slice is larger than the per-file cap, so the viewer can show
 * the reason instead of silently dropping it; the composer's own cap for the
 * session total is enforced when the attachment is added.
 */
export function attachmentFromDocument({
  content,
  path,
  root,
  startLine,
  endLine
}: DocumentSlice): FileAttachment {
  const lines = content.split('\n')
  const start = Math.max(1, startLine)
  const end = Math.max(start, endLine)
  const slice = lines.slice(start - 1, end)
  if (slice.length === 0) {
    throw new Error(`${basename(path)} line ${startLine} is past the end of the document`)
  }
  const text = slice.join('\n')
  const size = new TextEncoder().encode(text).length
  if (size > MAX_TEXT_FILE_BYTES) {
    throw new Error(
      `${basename(path)} lines ${start}-${end} are larger than ${Math.round(
        MAX_TEXT_FILE_BYTES / 1024
      )} KB`
    )
  }
  return {
    name: basename(path),
    path,
    absolutePath: root ? `${root.replace(/\/+$/, '')}/${path}` : undefined,
    startLine: start,
    endLine: end,
    contentBase64: bytesToBase64(new TextEncoder().encode(text)),
    size
  }
}
