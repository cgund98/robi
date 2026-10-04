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

export function isImageFile(file: File): boolean {
  return IMAGE_TYPES.has(file.type)
}

export function isTextFile(file: File): boolean {
  const dot = file.name.lastIndexOf('.')
  if (dot < 0 || dot === file.name.length - 1) {
    return false
  }
  return TEXT_EXTENSIONS.has(file.name.slice(dot + 1).toLowerCase())
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
