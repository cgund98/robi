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
