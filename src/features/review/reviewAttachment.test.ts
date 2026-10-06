import { describe, expect, it } from 'vitest'

import type { ReviewFile, ReviewHunk } from '../../api/review'
import { base64ToBytes } from '../chat/textAttachments'
import { hunkAttachment, rejectReasonInstruction, wholeFileAttachment } from './reviewAttachment'

function decode(base64: string): string {
  return new TextDecoder().decode(base64ToBytes(base64))
}

function file(overrides: Partial<ReviewFile>): ReviewFile {
  return {
    path: 'src/a.ts',
    status: 'modified',
    additions: 2,
    deletions: 2,
    baseline: 'a\nb\nc\nd\ne\n',
    current: 'a\nB\nc\nD\ne\n',
    lines: [],
    hunks: [],
    ...overrides
  }
}

function hunk(overrides: Partial<ReviewHunk>): ReviewHunk {
  return {
    id: 'first',
    old_start: 1,
    old_count: 1,
    new_start: 1,
    new_count: 1,
    ...overrides
  }
}

describe('wholeFileAttachment', () => {
  it('attaches the current body with no range and a workspace path', () => {
    const attachment = wholeFileAttachment(file({}), '/tmp/ws')
    expect(attachment.name).toBe('a.ts')
    expect(attachment.path).toBe('src/a.ts')
    expect(attachment.absolutePath).toBe('/tmp/ws/src/a.ts')
    expect(attachment.startLine).toBeUndefined()
    expect(attachment.endLine).toBeUndefined()
    expect(decode(attachment.contentBase64)).toBe('a\nB\nc\nD\ne\n')
    expect(attachment.size).toBe(decode(attachment.contentBase64).length)
  })

  it('drops the absolute path when the root is unknown', () => {
    const attachment = wholeFileAttachment(file({}), null)
    expect(attachment.absolutePath).toBeUndefined()
  })

  it('normalizes a trailing slash on the root', () => {
    expect(wholeFileAttachment(file({}), '/tmp/ws/').absolutePath).toBe('/tmp/ws/src/a.ts')
  })

  it('attaches the baseline for a file this session deleted', () => {
    const attachment = wholeFileAttachment(
      file({ status: 'deleted', current: '', baseline: 'a\nb\n' }),
      '/tmp/ws'
    )
    expect(decode(attachment.contentBase64)).toBe('a\nb\n')
    expect(attachment.startLine).toBeUndefined()
  })

  it('refuses a whole file over the per-file cap', () => {
    expect(() => wholeFileAttachment(file({ current: 'x\n'.repeat(40000) }), null)).toThrow(
      /larger than/
    )
  })
})

describe('hunkAttachment', () => {
  it('takes the range from new_start and new_count, 1-based and inclusive', () => {
    const attachment = hunkAttachment(file({}), hunk({ new_start: 1, new_count: 1 }), '/tmp/ws')
    expect(attachment.startLine).toBe(2)
    expect(attachment.endLine).toBe(2)
    expect(decode(attachment.contentBase64)).toBe('B')
  })

  it('spans every current line of a multi-line hunk', () => {
    const attachment = hunkAttachment(
      file({ current: 'a\nB\nBB\nc\n' }),
      hunk({ new_start: 1, new_count: 2 }),
      '/tmp/ws'
    )
    expect(attachment.startLine).toBe(2)
    expect(attachment.endLine).toBe(3)
    expect(decode(attachment.contentBase64)).toBe('B\nBB')
  })

  it('keeps the deletion point when the hunk only deletes', () => {
    const attachment = hunkAttachment(
      file({ baseline: 'a\nb\nc\n', current: 'a\nc\n' }),
      hunk({ old_start: 1, old_count: 1, new_start: 1, new_count: 0 }),
      '/tmp/ws'
    )
    expect(attachment.startLine).toBe(2)
    expect(attachment.endLine).toBe(2)
    expect(decode(attachment.contentBase64)).toBe('c')
  })

  it('falls back to the baseline lines when the deletion emptied the file', () => {
    const attachment = hunkAttachment(
      file({ status: 'deleted', baseline: 'a\nb\n', current: '' }),
      hunk({ old_start: 0, old_count: 2, new_start: 0, new_count: 0 }),
      '/tmp/ws'
    )
    expect(attachment.startLine).toBe(1)
    expect(attachment.endLine).toBe(2)
    expect(decode(attachment.contentBase64)).toBe('a\nb')
  })
})

describe('rejectReasonInstruction', () => {
  it('names the whole file with no range', () => {
    expect(rejectReasonInstruction('src/a.ts', null, 'keep the old name')).toBe(
      'I rejected the change to src/a.ts. Please iterate on the file using this feedback:\n\nkeep the old name'
    )
  })

  it('names a single line', () => {
    expect(rejectReasonInstruction('src/a.ts', { start: 4, end: 4 }, 'fix')).toContain(
      'src/a.ts (line 4)'
    )
  })

  it('names a line range', () => {
    expect(rejectReasonInstruction('src/a.ts', { start: 2, end: 5 }, 'fix')).toContain(
      'src/a.ts (lines 2-5)'
    )
  })
})
