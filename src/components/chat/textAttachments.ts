/** What an attachment chip needs: the name and, for a range, the lines. */
export type AttachmentMeta = {
  name: string
  /** Workspace-relative or absolute path, for display. Absent for an upload. */
  path?: string | null
  /** 1-based first line of the slice, when it was a range. */
  startLine?: number | null
  /** 1-based last line of the slice, inclusive, when it was a range. */
  endLine?: number | null
}

/** `base64` decoded to bytes. Standard alphabet. */
export function base64ToBytes(encoded: string): Uint8Array {
  const binary = atob(encoded)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

/**
 * One file the user attached, as the client holds it before sending.
 *
 * `contentBase64` is the file's raw bytes; the server decodes them, verifies
 * they are text, and decides from `absolutePath` whether the file is inside the
 * workspace. The client sniffs the bytes and checks the size for feedback, but
 * the server is the authority. `absolutePath` is set only by the desktop picker.
 */
export type FileAttachment = {
  name: string
  /**
   * The workspace-relative path, for display (the chip's tooltip). Set by a
   * producer that knows it, such as the docs viewer's line attach. Absent for a
   * picker or drop, whose chip falls back to `absolutePath`.
   */
  path?: string
  /** The file's absolute path, when the desktop picker provided one. */
  absolutePath?: string
  /** 1-based first line of the slice, when it was a range. */
  startLine?: number
  /** 1-based last line of the slice, inclusive, when it was a range. */
  endLine?: number
  contentBase64: string
  /** Decoded byte length, for the client's total-size check. */
  size: number
}

/** One file, matching the server's per-file cap. */
export const MAX_TEXT_FILE_BYTES = 64 * 1024

/** The sum of every attachment's bytes, matching the server's total cap. */
export const MAX_TEXT_TOTAL_BYTES = 256 * 1024

export const MAX_ATTACHMENTS = 8

const IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/webp', 'image/gif'])

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp', 'gif'])

function extension(name: string): string {
  const dot = name.lastIndexOf('.')
  if (dot < 0 || dot === name.length - 1) {
    return ''
  }
  return name.slice(dot + 1).toLowerCase()
}

export function isImageFile(file: File): boolean {
  if (IMAGE_TYPES.has(file.type)) {
    return true
  }
  // A drop from the desktop often arrives with an empty type. The extension
  // is enough for the types the paperclip already accepts.
  if (file.type && file.type !== 'application/octet-stream') {
    return false
  }
  return IMAGE_EXTENSIONS.has(extension(file.name))
}

/**
 * Whether `bytes` look like text, decided from content, not the name.
 *
 * The same rule the server and the read tools use: a NUL byte marks binary, and
 * the bytes must be valid UTF-8. There is no extension list — a `.log`, a
 * `.conf`, or an extensionless file is text if its bytes are. The server
 * re-checks; this is for immediate feedback at attach time.
 */
export function looksLikeText(bytes: Uint8Array): boolean {
  if (bytes.includes(0)) {
    return false
  }
  try {
    new TextDecoder('utf-8', { fatal: true }).decode(bytes)
    return true
  } catch {
    return false
  }
}

/** `bytes` as standard base64, chunked so a large file does not blow the stack. */
export function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  const chunk = 0x8000
  for (let index = 0; index < bytes.length; index += chunk) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunk))
  }
  return btoa(binary)
}

/**
 * Files from a paste or a drop.
 *
 * A screenshot paste often has no name. Give it one so the thumbnail and the
 * remove control have something to say.
 */
export function filesFromTransfer(data: DataTransfer | null): File[] {
  if (!data) {
    return []
  }
  const listed = Array.from(data.files)
  const files =
    listed.length > 0
      ? listed
      : Array.from(data.items)
          .filter((item) => item.kind === 'file')
          .map((item) => item.getAsFile())
          .filter((file): file is File => file !== null)
  return files.map(nameClipboardFile)
}

function nameClipboardFile(file: File): File {
  if (file.name) {
    return file
  }
  const subtype = file.type.split('/')[1]
  const ext = subtype === 'jpeg' ? 'jpg' : subtype || 'bin'
  return new File([file], `pasted.${ext}`, { type: file.type })
}

export function fileAccept(): string {
  // A hint for the picker, not a gate: the content decides. `text/*` covers the
  // files the OS labels, and a picker can still be switched to any file.
  return 'image/png,image/jpeg,image/webp,image/gif,text/*'
}

/**
 * Read one picked or dropped file into an attachment.
 *
 * An HTML file input exposes only the basename, so an attachment built here has
 * no `absolutePath` and the server treats it as outside the workspace. The
 * desktop picker (`attachmentFromPicked`) is the path-aware source. The bytes
 * are sniffed here so a binary file fails at attach time; the server re-checks.
 */
export async function readFileAttachment(file: File): Promise<FileAttachment> {
  if (file.size > MAX_TEXT_FILE_BYTES) {
    throw new Error(`${file.name} is larger than ${Math.round(MAX_TEXT_FILE_BYTES / 1024)} KB`)
  }
  const bytes = new Uint8Array(await file.arrayBuffer())
  if (!looksLikeText(bytes)) {
    throw new Error(`${file.name} is not a text file`)
  }
  return { name: file.name, contentBase64: bytesToBase64(bytes), size: bytes.length }
}

/**
 * An attachment from one file the native desktop picker returned.
 *
 * The bytes are already read by the shell; this sniffs them for feedback and
 * keeps the absolute path, which the server turns into a workspace-relative path
 * or drops (outside).
 */
export function attachmentFromPicked(picked: {
  name: string
  absolutePath: string
  contentBase64: string
}): FileAttachment {
  const bytes = base64ToBytes(picked.contentBase64)
  if (!looksLikeText(bytes)) {
    throw new Error(`${picked.name} is not a text file`)
  }
  return {
    name: picked.name,
    absolutePath: picked.absolutePath,
    contentBase64: picked.contentBase64,
    size: bytes.length
  }
}

/** The `files` array for `POST /messages`, in the server's shape. */
export function toFileInputs(files: FileAttachment[]): {
  name: string
  absolute_path?: string
  start_line?: number
  end_line?: number
  content_base64: string
}[] {
  return files.map((file) => {
    const input: {
      name: string
      absolute_path?: string
      start_line?: number
      end_line?: number
      content_base64: string
    } = { name: file.name, content_base64: file.contentBase64 }
    if (file.absolutePath != null) {
      input.absolute_path = file.absolutePath
    }
    if (file.startLine != null) {
      input.start_line = file.startLine
    }
    if (file.endLine != null) {
      input.end_line = file.endLine
    }
    return input
  })
}
