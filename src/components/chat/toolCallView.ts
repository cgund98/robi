import type { components } from '../../api/schema'

export type ChatToolCall = components['schemas']['ChatToolCall']

export type ToolSummary = {
  verb: string
  target: string
  /** Inclusive line window, such as `L240-299`, when the call names one. */
  range?: string
}

export function toolSummary(call: ChatToolCall): ToolSummary {
  const args = record(call.args)
  const path = stringField(args, 'path')
  const pattern = stringField(args, 'pattern')
  switch (call.name) {
    case 'read_file':
    case 'read_code': {
      const range = readLineRange(args)
      return { verb: 'Read', target: path || 'file', ...(range ? { range } : {}) }
    }
    case 'grep':
      return { verb: 'Grepped', target: pattern || path || 'workspace' }
    case 'semantic_search':
      return { verb: 'Searched', target: stringField(args, 'query') || 'workspace' }
    case 'web_search':
      return { verb: 'Search', target: stringField(args, 'query') || 'the web' }
    case 'web_fetch':
      return { verb: 'Fetch', target: stringField(args, 'url') || 'a page' }
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
    case 'shell':
      return {
        verb: args?.unsandboxed === true ? 'Run unsandboxed' : 'Run',
        target: shellProgram(stringField(args, 'command'))
      }
    case 'retrieve':
      return retrieveSummary(call)
    case 'delegate':
      return {
        verb: 'Delegate',
        target: stringField(args, 'description') || stringField(args, 'task')
      }
    case 'write_plan':
      return { verb: 'Plan', target: planTitle(call) }
    case 'todos':
      return { verb: 'Update', target: 'tasks' }
    default:
      if (call.name.startsWith('mcp_')) {
        return { verb: 'MCP call', target: call.name.slice('mcp_'.length) }
      }
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
  | { kind: 'shell'; command: string; output: string }
  | { kind: 'retrieve'; view: RetrieveView }
  | { kind: 'mcp'; args: string; output: string }
  | { kind: 'error'; text: string }

export function toolDetail(call: ChatToolCall): ToolDetail | null {
  if (call.name.startsWith('mcp_')) {
    const result = typeof call.result === 'string' ? call.result : ''
    return {
      kind: 'mcp',
      args: JSON.stringify(call.args ?? {}, null, 2),
      output: [result, call.error ?? ''].filter((text) => text.length > 0).join('\n')
    }
  }
  if (call.name === 'retrieve') {
    return { kind: 'retrieve', view: retrieveView(call) }
  }
  if (call.name === 'shell') {
    const command = stringField(record(call.args), 'command')
    const result = record(call.result)
    const stdout = typeof result?.stdout === 'string' ? result.stdout : ''
    const stderr = typeof result?.stderr === 'string' ? result.stderr : ''
    const output = [stdout, stderr, call.error ?? ''].filter((text) => text.length > 0).join('\n')
    return { kind: 'shell', command, output }
  }
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

export type RetrieveView = {
  id: string
  stream: 'stdout' | 'stderr' | 'both'
  exitCode: number | null
  truncated: boolean
  startLine: number
  endLine: number
  totalLines: number
  nextOffset: number | null
  stdout: string
  stderr: string
  raw: boolean
}

function retrieveSummary(call: ChatToolCall): ToolSummary {
  const view = retrieveView(call)
  const range =
    view.totalLines > 0 && view.endLine >= view.startLine
      ? view.startLine === view.endLine
        ? `L${view.startLine}`
        : `L${view.startLine}-${view.endLine}`
      : undefined
  const stream = view.stream === 'both' ? '' : view.stream
  const target = [shortId(view.id), stream].filter((part) => part.length > 0).join(' ')
  return { verb: 'Retrieved', target: target || 'log', ...(range ? { range } : {}) }
}

function retrieveView(call: ChatToolCall): RetrieveView {
  const args = record(call.args)
  const result = record(call.result)
  const stream = streamField(args?.stream)
  return {
    id: stringField(args, 'id'),
    stream,
    exitCode: numberOrNull(result, 'exit_code'),
    truncated: result?.truncated === true,
    startLine: numberField(result, 'start_line') || 1,
    endLine: numberField(result, 'end_line'),
    totalLines: numberField(result, 'total_lines'),
    nextOffset: numberOrNull(result, 'next_offset'),
    stdout: typeof result?.stdout === 'string' ? result.stdout : '',
    stderr: typeof result?.stderr === 'string' ? result.stderr : '',
    raw: args?.raw === true
  }
}

function streamField(value: unknown): 'stdout' | 'stderr' | 'both' {
  if (value === 'stdout' || value === 'stderr') {
    return value
  }
  return 'both'
}

function shortId(id: string): string {
  const hex = id.replace(/-/g, '')
  if (hex.length >= 8) {
    return hex.slice(0, 8)
  }
  return id
}

function numberOrNull(value: Record<string, unknown> | null, key: string): number | null {
  const field = value?.[key]
  return typeof field === 'number' && Number.isFinite(field) ? field : null
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

function shellProgram(command: string): string {
  const token = command.trim().split(/\s+/)[0] ?? ''
  const name = token.split('/').pop() ?? ''
  return name || 'command'
}

export type SubagentStepView = {
  name: string
  target: string
  status: 'running' | 'ok' | 'denied' | 'failed'
}

export type SubagentView = {
  mode: 'explore' | 'general'
  description: string
  startedMs: number
  steps: SubagentStepView[]
  answer: string
}

/** The child run on a `delegate` call, when the snapshot has arrived. */
export function subagentView(call: ChatToolCall): SubagentView | null {
  if (call.name !== 'delegate' || !call.subagent) {
    return null
  }
  const mode = call.subagent.mode === 'general' ? 'general' : 'explore'
  const steps = call.subagent.steps.map((step) => ({
    name: step.name,
    target: step.target,
    status: stepStatus(step.status)
  }))
  return {
    mode,
    description: call.subagent.description,
    startedMs: call.subagent.started_ms,
    steps,
    answer: stringField(record(call.result), 'answer')
  }
}

/** The collapsed explore row: `Exploring 9 files, 5 searches`. */
export function exploreSummary(view: SubagentView): string {
  const files = new Set(
    view.steps
      .filter(
        (step) =>
          (step.name === 'read_file' || step.name === 'read_code') && step.target.length > 0
      )
      .map((step) => step.target)
  ).size
  const searches = view.steps.filter((step) => step.name === 'grep' || step.name === 'find').length
  const parts: string[] = []
  if (files === 1) {
    parts.push('1 file')
  } else if (files > 1) {
    parts.push(`${files} files`)
  }
  if (searches === 1) {
    parts.push('1 search')
  } else if (searches > 1) {
    parts.push(`${searches} searches`)
  }
  if (parts.length === 0) {
    return 'Exploring'
  }
  return `Exploring ${parts.join(', ')}`
}

export function subagentCount(view: SubagentView): string {
  const searches = view.steps.filter((step) => step.name === 'grep' || step.name === 'find').length
  if (view.mode === 'explore' && searches > 0) {
    return searches === 1 ? '1 search' : `${searches} searches`
  }
  if (view.steps.length === 0) {
    return 'starting'
  }
  return view.steps.length === 1 ? '1 tool call' : `${view.steps.length} tool calls`
}

export function subagentStepSummary(step: SubagentStepView): ToolSummary {
  if (step.name === 'shell') {
    return { verb: 'Run', target: step.target }
  }
  const args =
    step.name === 'grep' || step.name === 'find' ? { pattern: step.target } : { path: step.target }
  return toolSummary({
    id: '',
    name: step.name,
    args,
    approval_status: 'pending',
    execution_status: 'succeeded'
  })
}

function stepStatus(status: string): SubagentStepView['status'] {
  if (status === 'ok' || status === 'denied' || status === 'failed' || status === 'running') {
    return status
  }
  return 'failed'
}

export type PlanTodoStatus = 'pending' | 'in_progress' | 'completed' | 'canceled'

export type PlanTodo = {
  id: string
  content: string
  status: PlanTodoStatus
}

export type PlanView = {
  title: string
  summary: string
  body: string
  path: string
  created: boolean
  todos: PlanTodo[]
}

const PLAN_TODO_STATUSES: readonly PlanTodoStatus[] = [
  'pending',
  'in_progress',
  'completed',
  'canceled'
]

/** The markdown a finished `write_plan` saved, ready to open again. */
export function planView(call: ChatToolCall): PlanView | null {
  if (call.name !== 'write_plan' || call.error || call.execution_status !== 'succeeded') {
    return null
  }
  const args = record(call.args)
  const body = stringField(args, 'body')
  const title = planTitle(call)
  const path = stringField(record(call.result), 'path') || stringField(args, 'path')
  return {
    title,
    summary: planSummary(body, title),
    body,
    path,
    created: stringField(record(call.result), 'status') !== 'updated',
    todos: parsePlanTodos(args?.todos)
  }
}

/** Todo steps from a plan write or a `todos` result. Steps with no content are left off. */
export function parsePlanTodos(value: unknown): PlanTodo[] {
  if (!Array.isArray(value)) {
    return []
  }
  const todos: PlanTodo[] = []
  for (const item of value) {
    const fields = record(item)
    if (!fields) {
      continue
    }
    const id = stringField(fields, 'id').trim()
    const content = stringField(fields, 'content').trim()
    const raw = stringField(fields, 'status').trim() || 'pending'
    if (!id || !content || !isPlanTodoStatus(raw)) {
      continue
    }
    todos.push({ id, content, status: raw })
  }
  return todos
}

function isPlanTodoStatus(value: string): value is PlanTodoStatus {
  return PLAN_TODO_STATUSES.some((status) => status === value)
}

/** The user message that asks agent mode to carry out a saved plan. */
export function planBuildInstruction(path: string): string {
  return `Implement the plan at ${path}. Read that file and make the changes it describes.`
}

function planTitle(call: ChatToolCall): string {
  const args = record(call.args)
  const path = stringField(record(call.result), 'path') || stringField(args, 'path')
  const heading = firstHeading(stringField(args, 'body'))
  if (heading) {
    return heading
  }
  const name = stringField(args, 'plan_name').trim()
  if (name) {
    return name
  }
  const file = path.split('/').pop() ?? ''
  const stem = file.replace(/\.md$/i, '')
  return stem || 'plan'
}

function planSummary(body: string, title: string): string {
  let fenced = false
  const paragraphs: string[] = []
  let current: string[] = []
  const flush = () => {
    const text = current.join(' ').replace(/\s+/g, ' ').trim()
    current = []
    if (text && text !== title) {
      paragraphs.push(text)
    }
  }
  for (const line of body.split('\n')) {
    const trimmed = line.trim()
    if (trimmed.startsWith('```')) {
      if (fenced) {
        flush()
      }
      fenced = !fenced
      continue
    }
    if (fenced || /^#{1,6}\s+/.test(trimmed)) {
      flush()
      continue
    }
    if (!trimmed) {
      flush()
      continue
    }
    current.push(trimmed.replace(/^[-*]\s+/, '').replace(/^\d+\.\s+/, ''))
  }
  flush()
  return paragraphs[0] ?? ''
}

function firstHeading(body: string): string {
  let fenced = false
  for (const line of body.split('\n')) {
    const trimmed = line.trim()
    if (trimmed.startsWith('```')) {
      fenced = !fenced
      continue
    }
    if (fenced) {
      continue
    }
    const match = /^#{1,6}\s+(\S.*?)\s*#*\s*$/.exec(line)
    if (match) {
      return match[1].trim()
    }
  }
  return ''
}

function stringField(value: Record<string, unknown> | null, key: string): string {
  const field = value?.[key]
  return typeof field === 'string' ? field : ''
}

/** `L240-299` from a `read_file` offset and limit. Empty when the call reads the whole file. */
function readLineRange(args: Record<string, unknown> | null): string {
  const offset = positiveInt(args, 'offset')
  const limit = positiveInt(args, 'limit')
  if (offset && limit) {
    return `L${offset}-${offset + limit - 1}`
  }
  if (limit) {
    return `L1-${limit}`
  }
  if (offset) {
    return `L${offset}`
  }
  return ''
}

function positiveInt(value: Record<string, unknown> | null, key: string): number | null {
  const field = value?.[key]
  if (typeof field !== 'number' || !Number.isInteger(field) || field < 1) {
    return null
  }
  return field
}
