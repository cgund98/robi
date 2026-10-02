import type { components } from '../../api/schema'

export type ChatToolCall = components['schemas']['ChatToolCall']

export type ToolSummary = {
  verb: string
  target: string
}

export function toolSummary(call: ChatToolCall): ToolSummary {
  const args = record(call.args)
  const path = stringField(args, 'path')
  const pattern = stringField(args, 'pattern')
  switch (call.name) {
    case 'read_file':
      return { verb: 'Read', target: path || 'file' }
    case 'grep':
      return { verb: 'Grepped', target: pattern || path || 'workspace' }
    case 'find':
      return { verb: 'Found', target: pattern || path || 'files' }
    case 'list_dir':
      return { verb: 'Listed', target: path || '.' }
    case 'grant':
      return { verb: 'Grant', target: path || 'path' }
    case 'write_file':
      return { verb: 'Write', target: path || 'file' }
    case 'edit_file':
      return { verb: 'Edit', target: path || 'file' }
    case 'delete_file':
      return { verb: 'Delete', target: path || 'file' }
    default:
      return { verb: call.name, target: path || pattern }
  }
}

/** A paused call the user still has to accept or refuse. */
export function needsDecision(
  call: ChatToolCall,
  phase: 'idle' | 'thinking' | 'responding'
): boolean {
  return (
    phase === 'idle' &&
    call.approval_status === 'pending' &&
    call.execution_status === 'not_started'
  )
}

export function hasDetail(call: ChatToolCall): boolean {
  return call.error != null || call.result != null
}

/** Diff lines an edit card shows before the user opens the rest. */
export const EDIT_VISIBLE_LINES = 4

/** How many diff lines an opened edit card shows before it stops. */
export const EDIT_PREVIEW_LINES = 24

const EDIT_TOOLS = new Set(['write_file', 'edit_file', 'delete_file'])

export type DiffLine = {
  kind: 'context' | 'add' | 'del'
  lineNo: number
  text: string
}

export type EditPreview = {
  path: string
  fullPath: string
  additions: number
  deletions: number
  lines: DiffLine[]
  hidden: number
}

/** The change an `edit_file` approval is asking to make, from `old` and `new`. */
export function editSuggestion(call: ChatToolCall): EditPreview | null {
  if (call.name !== 'edit_file') {
    return null
  }
  const args = record(call.args)
  const path = stringField(args, 'path')
  if (!path || !args || typeof args.old !== 'string' || typeof args.new !== 'string') {
    return null
  }
  const all = diffSnippet(args.old, args.new)
  return {
    path,
    fullPath: path,
    additions: all.filter((line) => line.kind === 'add').length,
    deletions: all.filter((line) => line.kind === 'del').length,
    lines: all.slice(0, EDIT_PREVIEW_LINES),
    hidden: Math.max(0, all.length - EDIT_PREVIEW_LINES)
  }
}

/** The collapsed edit row and the short diff it opens. */
export function editPreview(call: ChatToolCall): EditPreview | null {
  if (!EDIT_TOOLS.has(call.name) || call.error) {
    return null
  }
  const result = record(call.result)
  if (!result) {
    return null
  }
  const argsPath = stringField(record(call.args), 'path')
  const resultPath = stringField(result, 'path')
  const path = argsPath || resultPath
  if (!path) {
    return null
  }
  const parsed = parseUnified(stringField(result, 'patch'))
  const all = parsed.length > 0 ? parsed : linesFromHunks(result.hunks)
  return {
    path,
    fullPath: resultPath || path,
    additions: numberField(result, 'additions'),
    deletions: numberField(result, 'deletions'),
    lines: all.slice(0, EDIT_PREVIEW_LINES),
    hidden: Math.max(0, all.length - EDIT_PREVIEW_LINES)
  }
}

function splitLines(text: string): string[] {
  if (!text) {
    return []
  }
  const lines: string[] = []
  let rest = text
  while (rest.includes('\n')) {
    const index = rest.indexOf('\n')
    lines.push(rest.slice(0, index).replace(/\r$/, ''))
    rest = rest.slice(index + 1)
  }
  if (rest) {
    lines.push(rest.replace(/\r$/, ''))
  }
  return lines
}

/** Line diff of one replacement. Long snippets are shown as a full replace. */
function diffSnippet(before: string, after: string): DiffLine[] {
  const oldLines = splitLines(before)
  const newLines = splitLines(after)
  if (oldLines.length > 400 || newLines.length > 400) {
    return [
      ...oldLines.map((text, index) => ({ kind: 'del' as const, lineNo: index + 1, text })),
      ...newLines.map((text, index) => ({ kind: 'add' as const, lineNo: index + 1, text }))
    ]
  }
  const score = Array.from({ length: oldLines.length + 1 }, () =>
    new Array<number>(newLines.length + 1).fill(0)
  )
  for (let i = oldLines.length - 1; i >= 0; i -= 1) {
    for (let j = newLines.length - 1; j >= 0; j -= 1) {
      score[i][j] =
        oldLines[i] === newLines[j]
          ? score[i + 1][j + 1] + 1
          : Math.max(score[i + 1][j], score[i][j + 1])
    }
  }
  const lines: DiffLine[] = []
  let i = 0
  let j = 0
  let oldNo = 1
  let newNo = 1
  while (i < oldLines.length && j < newLines.length) {
    if (oldLines[i] === newLines[j]) {
      lines.push({ kind: 'context', lineNo: newNo, text: oldLines[i] })
      i += 1
      j += 1
      oldNo += 1
      newNo += 1
    } else if (score[i + 1][j] >= score[i][j + 1]) {
      lines.push({ kind: 'del', lineNo: oldNo, text: oldLines[i] })
      i += 1
      oldNo += 1
    } else {
      lines.push({ kind: 'add', lineNo: newNo, text: newLines[j] })
      j += 1
      newNo += 1
    }
  }
  while (i < oldLines.length) {
    lines.push({ kind: 'del', lineNo: oldNo, text: oldLines[i] })
    i += 1
    oldNo += 1
  }
  while (j < newLines.length) {
    lines.push({ kind: 'add', lineNo: newNo, text: newLines[j] })
    j += 1
    newNo += 1
  }
  return lines
}

function parseUnified(patch: string): DiffLine[] {
  if (!patch) {
    return []
  }
  const lines: DiffLine[] = []
  let oldLine = 0
  let newLine = 0
  for (const raw of patch.split('\n')) {
    const header = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(raw)
    if (header) {
      oldLine = Number(header[1])
      newLine = Number(header[2])
      continue
    }
    if (
      raw.startsWith('---') ||
      raw.startsWith('+++') ||
      raw.startsWith('diff ') ||
      raw.startsWith('\\')
    ) {
      continue
    }
    if (raw.startsWith('-')) {
      lines.push({ kind: 'del', lineNo: oldLine, text: raw.slice(1) })
      oldLine += 1
      continue
    }
    if (raw.startsWith('+')) {
      lines.push({ kind: 'add', lineNo: newLine, text: raw.slice(1) })
      newLine += 1
      continue
    }
    if (raw.startsWith(' ')) {
      lines.push({ kind: 'context', lineNo: newLine, text: raw.slice(1) })
      oldLine += 1
      newLine += 1
    }
  }
  return lines
}

function linesFromHunks(value: unknown): DiffLine[] {
  if (!Array.isArray(value)) {
    return []
  }
  const lines: DiffLine[] = []
  for (const hunk of value) {
    const row = record(hunk)
    if (!row) {
      continue
    }
    const oldStart = numberField(row, 'old_start')
    const newStart = numberField(row, 'new_start')
    stringList(row.old).forEach((text, index) => {
      lines.push({ kind: 'del', lineNo: oldStart + index + 1, text })
    })
    stringList(row.new).forEach((text, index) => {
      lines.push({ kind: 'add', lineNo: newStart + index + 1, text })
    })
  }
  return lines
}

function stringList(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return []
  }
  return value.filter((line): line is string => typeof line === 'string')
}

function numberField(value: Record<string, unknown> | null, key: string): number {
  const field = value?.[key]
  return typeof field === 'number' && Number.isFinite(field) ? field : 0
}

export type ToolDetail =
  | { kind: 'code'; startLine: number; lines: string[] }
  | { kind: 'lines'; lines: string[] }
  | { kind: 'error'; text: string }

export function toolDetail(call: ChatToolCall): ToolDetail | null {
  if (call.error) {
    return { kind: 'error', text: call.error }
  }
  const result = record(call.result)
  if (!result) {
    return null
  }
  if (typeof result.content === 'string') {
    const start = typeof result.start_line === 'number' ? result.start_line : 1
    return {
      kind: 'code',
      startLine: start,
      lines: result.content.length === 0 ? [] : result.content.split('\n')
    }
  }
  if (Array.isArray(result.matches)) {
    return {
      kind: 'lines',
      lines: result.matches.map((match) => formatMatch(record(match)))
    }
  }
  if (Array.isArray(result.files)) {
    return {
      kind: 'lines',
      lines: result.files.map((file) => (typeof file === 'string' ? file : JSON.stringify(file)))
    }
  }
  if (Array.isArray(result.entries)) {
    return {
      kind: 'lines',
      lines: result.entries.map((entry) => {
        const row = record(entry)
        const name = stringField(row, 'name')
        const kind = stringField(row, 'kind')
        return kind ? `${name}  ${kind}` : name
      })
    }
  }
  return { kind: 'lines', lines: [JSON.stringify(result, null, 2)] }
}

function formatMatch(match: Record<string, unknown> | null): string {
  if (!match) {
    return ''
  }
  const path = stringField(match, 'path')
  const line = typeof match.line === 'number' ? String(match.line) : ''
  const text = stringField(match, 'text')
  return `${path}:${line}: ${text}`.trim()
}

function record(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as Record<string, unknown>
  }
  return null
}

function stringField(value: Record<string, unknown> | null, key: string): string {
  const field = value?.[key]
  return typeof field === 'string' ? field : ''
}
