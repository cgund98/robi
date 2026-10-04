/** Text files the paperclip will read and fold into the instruction. */
const TEXT_EXTENSIONS = new Set([
  'c',
  'cc',
  'cpp',
  'css',
  'csv',
  'env',
  'go',
  'h',
  'hpp',
  'html',
  'ini',
  'java',
  'js',
  'json',
  'jsx',
  'kt',
  'less',
  'lock',
  'md',
  'php',
  'py',
  'rb',
  'rs',
  'scss',
  'sh',
  'sql',
  'svelte',
  'swift',
  'toml',
  'ts',
  'tsx',
  'txt',
  'vue',
  'xml',
  'yaml',
  'yml'
])

/** One text file, matching the 10 MiB image cap would swamp a context window. */
export const MAX_TEXT_FILE_BYTES = 256 * 1024

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

export function isTextFile(file: File): boolean {
  return TEXT_EXTENSIONS.has(extension(file.name))
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
  const extensions = [...TEXT_EXTENSIONS].map((ext) => `.${ext}`).join(',')
  return `image/png,image/jpeg,image/webp,image/gif,${extensions}`
}

/** Read a text attachment. Rejects oversized files and files that contain a NUL. */
export async function readTextFile(file: File): Promise<string> {
  if (file.size > MAX_TEXT_FILE_BYTES) {
    throw new Error(`${file.name} is larger than 256 KB`)
  }
  const text = await file.text()
  if (text.includes('\0')) {
    throw new Error(`${file.name} is not a text file`)
  }
  return text
}

/**
 * Append each file after the draft so the model sees the bytes as text.
 * A message that is only attachments still has a body.
 */
export function instructionWithTextFiles(
  draft: string,
  files: { name: string; text: string }[]
): string {
  if (files.length === 0) {
    return draft
  }
  const blocks = files
    .map((file) => `<file name="${file.name}">\n${file.text}\n</file>`)
    .join('\n\n')
  const trimmed = draft.trim()
  return trimmed.length > 0 ? `${trimmed}\n\n${blocks}` : blocks
}
