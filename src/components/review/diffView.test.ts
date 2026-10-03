import { describe, expect, it } from 'vitest'

import type { ReviewHunk, ReviewLine } from '../../api/review'
import { chunksFor, expandReviewLines, linesForView, reviewNote } from './diffView'

function line(kind: ReviewLine['kind'], text: string): ReviewLine {
  return { kind, text, old_line: null, new_line: null }
}

const sample: ReviewLine[] = [line('context', 'keep'), line('delete', 'old'), line('insert', 'new')]

describe('linesForView', () => {
  it('hides deletions in Current and insertions in Previous', () => {
    expect(linesForView(sample, 'current').map((item) => item.text)).toEqual(['keep', 'new'])
    expect(linesForView(sample, 'previous').map((item) => item.text)).toEqual(['keep', 'old'])
    expect(linesForView(sample, 'diff').map((item) => item.kind)).toEqual([
      'context',
      'delete',
      'insert'
    ])
  })

  it('notes an added file with the after side hidden, and a deleted file with the before side hidden', () => {
    const added = linesForView([line('insert', 'fn')], 'previous')
    expect(reviewNote('added', 'previous', added)).toBe('File added')
    const deleted = linesForView([line('delete', 'fn')], 'current')
    expect(reviewNote('deleted', 'current', deleted)).toBe('File deleted')
    expect(reviewNote('modified', 'current', linesForView(sample, 'current'))).toBeNull()
  })
})

describe('expandReviewLines', () => {
  it('puts the omitted lines back, including the ends of the file', () => {
    const baseline = 'a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\n'
    const current = 'a\nb\nc\nD\ne\nf\ng\nh\ni\nj\nk\nL\nm\nn\no\n'
    const truncated: ReviewLine[] = [
      { kind: 'context', text: 'c', old_line: 3, new_line: 3 },
      { kind: 'delete', text: 'd', old_line: 4, new_line: null },
      { kind: 'insert', text: 'D', old_line: null, new_line: 4 },
      { kind: 'context', text: 'e', old_line: 5, new_line: 5 },
      { kind: 'context', text: 'f', old_line: 6, new_line: 6 },
      { kind: 'context', text: 'g', old_line: 7, new_line: 7 },
      { kind: 'gap', text: '', old_line: null, new_line: null },
      { kind: 'context', text: 'i', old_line: 9, new_line: 9 },
      { kind: 'context', text: 'j', old_line: 10, new_line: 10 },
      { kind: 'context', text: 'k', old_line: 11, new_line: 11 },
      { kind: 'delete', text: 'l', old_line: 12, new_line: null },
      { kind: 'insert', text: 'L', old_line: null, new_line: 12 },
      { kind: 'context', text: 'm', old_line: 13, new_line: 13 }
    ]
    const full = expandReviewLines(truncated, baseline, current)
    expect(full.map((line) => line.kind)).not.toContain('gap')
    expect(full.map((line) => line.text)).toEqual([
      'a',
      'b',
      'c',
      'd',
      'D',
      'e',
      'f',
      'g',
      'h',
      'i',
      'j',
      'k',
      'l',
      'L',
      'm',
      'n',
      'o'
    ])
    expect(full.find((line) => line.text === 'h')).toMatchObject({
      kind: 'context',
      old_line: 8,
      new_line: 8
    })
  })
})

describe('chunksFor', () => {
  it('splits on gaps and keeps the hunk that owns each side', () => {
    const lines: ReviewLine[] = [
      { kind: 'delete', text: 'old', old_line: 2, new_line: null },
      { kind: 'insert', text: 'new', old_line: null, new_line: 2 },
      { kind: 'gap', text: '', old_line: null, new_line: null },
      { kind: 'insert', text: 'tail', old_line: null, new_line: 20 }
    ]
    const hunks: ReviewHunk[] = [
      { id: 'first', old_start: 1, old_count: 1, new_start: 1, new_count: 1 },
      { id: 'second', old_start: 19, old_count: 0, new_start: 19, new_count: 1 }
    ]
    const chunks = chunksFor(lines, hunks)
    expect(chunks.map((chunk) => chunk.hunkIds)).toEqual([['first'], ['second']])
  })
})
