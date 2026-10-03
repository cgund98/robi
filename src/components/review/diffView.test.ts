import { describe, expect, it } from 'vitest'

import type { ReviewHunk, ReviewLine } from '../../api/review'
import { chunksFor, linesForView, reviewNote } from './diffView'

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
