import { describe, expect, it } from 'vitest'

import {
  EDIT_PREVIEW_LINES,
  editPreview,
  editSuggestion,
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
    expect(toolSummary(call({ name: 'edit_file', args: { path: 'src/main.rs' } }))).toEqual({
      verb: 'Edit',
      target: 'src/main.rs'
    })
    expect(toolSummary(call({ name: 'delete_file', args: { path: '.env' } }))).toEqual({
      verb: 'Delete',
      target: '.env'
    })
    expect(toolSummary(call({ name: 'shell', args: { command: 'cargo test' } }))).toEqual({
      verb: 'Run',
      target: 'cargo'
    })
    expect(
      toolSummary(call({ name: 'shell', args: { command: 'gh pr view', unsandboxed: true } }))
    ).toEqual({
      verb: 'Run unsandboxed',
      target: 'gh'
    })
    expect(
      toolSummary(
        call({ name: 'shell', args: { command: '/usr/bin/curl -I https://example.com' } })
      )
    ).toEqual({
      verb: 'Run',
      target: 'curl'
    })
  })
})

describe('toolDetail', () => {
  it('keeps a shell command separate from its output', () => {
    expect(
      toolDetail(
        call({
          name: 'shell',
          args: { command: 'make lint' },
          result: { stdout: 'cargo fmt\n', stderr: '', sandboxed: true }
        })
      )
    ).toEqual({ kind: 'shell', command: 'make lint', output: 'cargo fmt\n' })
  })
})

describe('editPreview', () => {
  const patch = [
    '--- a/src/replace.rs',
    '+++ b/src/replace.rs',
    '@@ -120,4 +120,4 @@',
    ' fn a_whitespace_near_miss_says_so() {',
    '-    let error = apply_edit(" beta\\n", "beta\\n", "gamma\\n", false).unwrap_err();',
    '+    let error = apply_edit("  beta\\n", " beta\\n", "gamma\\n", false).unwrap_err();',
    '     assert!(error.contains("whitespace"), "{error}");',
    ...Array.from({ length: 30 }, (_, index) => `+    let extra_${index} = 1;`)
  ].join('\n')

  it('keeps the called path, the counts, and the first lines of the diff', () => {
    const preview = editPreview(
      call({
        name: 'edit_file',
        args: { path: 'src/replace.rs' },
        result: { path: 'src/replace.rs', additions: 31, deletions: 1, patch }
      })
    )
    expect(preview?.path).toBe('src/replace.rs')
    expect(preview?.fullPath).toBe('src/replace.rs')
    expect(preview?.additions).toBe(31)
    expect(preview?.deletions).toBe(1)
    expect(preview?.lines).toHaveLength(EDIT_PREVIEW_LINES)
    expect(preview?.hidden).toBe(10)
    expect(preview?.lines[0]).toMatchObject({ kind: 'context', lineNo: 120 })
    expect(preview?.lines[1]).toMatchObject({ kind: 'del', lineNo: 121 })
    expect(preview?.lines[2]).toMatchObject({ kind: 'add', lineNo: 121 })
  })

  it('shows the path the tool was called with', () => {
    const preview = editPreview(
      call({
        name: 'write_file',
        args: { path: '/tmp/test.md' },
        result: {
          path: '../../../../../private/tmp/test.md',
          additions: 3,
          deletions: 0,
          patch: ''
        }
      })
    )
    expect(preview?.path).toBe('/tmp/test.md')
    expect(preview?.fullPath).toBe('../../../../../private/tmp/test.md')

    const outside = editPreview(
      call({
        name: 'edit_file',
        args: { path: '../gopi/test.md' },
        result: { path: '../gopi/test.md', additions: 0, deletions: 0, patch: '' }
      })
    )
    expect(outside?.path).toBe('../gopi/test.md')
  })
})

describe('editSuggestion', () => {
  it('diffs the proposed replacement and stops at the same line cap', () => {
    const preview = editSuggestion(
      call({
        name: 'edit_file',
        execution_status: 'not_started',
        args: {
          path: 'src/Transcript.module.css',
          old: 'color: var(--ink);\n',
          new: 'color: var(--ink-bright);\n'
        }
      })
    )
    expect(preview?.path).toBe('src/Transcript.module.css')
    expect(preview?.additions).toBe(1)
    expect(preview?.deletions).toBe(1)
    expect(preview?.lines.map((line) => line.kind)).toEqual(['del', 'add'])

    const long = editSuggestion(
      call({
        name: 'edit_file',
        execution_status: 'not_started',
        args: {
          path: 'src/main.rs',
          old: 'keep\n',
          new: ['keep', ...Array.from({ length: 40 }, () => 'extra')].join('\n')
        }
      })
    )
    expect(long?.lines).toHaveLength(EDIT_PREVIEW_LINES)
    expect(long?.hidden).toBe(40 + 1 - EDIT_PREVIEW_LINES)
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
