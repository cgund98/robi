import { describe, expect, it } from 'vitest'

import {
  hasDetail,
  needsDecision,
  toolDetail,
  toolSummary,
  type ChatToolCall
} from './toolCallView'

function call(overrides: Partial<ChatToolCall> = {}): ChatToolCall {
  return {
    id: '1',
    name: 'read_file',
    args: { path: 'src/main.rs' },
    approval_status: 'pending',
    execution_status: 'succeeded',
    ...overrides
  }
}

describe('toolSummary', () => {
  it('names read, grep, find, and list', () => {
    expect(toolSummary(call())).toEqual({ verb: 'Read', target: 'src/main.rs' })
    expect(toolSummary(call({ name: 'grep', args: { pattern: 'fn main' } }))).toEqual({
      verb: 'Grepped',
      target: 'fn main'
    })
    expect(toolSummary(call({ name: 'find', args: { pattern: '*.rs' } }))).toEqual({
      verb: 'Found',
      target: '*.rs'
    })
    expect(toolSummary(call({ name: 'list_dir', args: {} }))).toEqual({
      verb: 'Listed',
      target: '.'
    })
    expect(toolSummary(call({ name: 'grant', args: { path: '.env', access: 'read' } }))).toEqual({
      verb: 'Grant',
      target: '.env'
    })
  })
})

describe('needsDecision', () => {
  it('is only a paused call that has not started', () => {
    const waiting = call({ execution_status: 'not_started' })
    expect(needsDecision(waiting, 'idle')).toBe(true)
    expect(needsDecision(waiting, 'thinking')).toBe(false)
    expect(needsDecision(call({ execution_status: 'succeeded' }), 'idle')).toBe(false)
    expect(
      needsDecision(call({ approval_status: 'approved', execution_status: 'not_started' }), 'idle')
    ).toBe(false)
  })
})

describe('toolDetail', () => {
  it('prefers the error and numbers read_file lines', () => {
    expect(toolDetail(call({ error: 'denied' }))).toEqual({ kind: 'error', text: 'denied' })
    expect(hasDetail(call({ error: 'denied' }))).toBe(true)
    expect(
      toolDetail(
        call({
          result: { content: 'a\nb', start_line: 4 }
        })
      )
    ).toEqual({ kind: 'code', startLine: 4, lines: ['a', 'b'] })
  })
})
