import { describe, expect, it } from 'vitest'

import { attachmentFromDocument } from './docAttachment'
import { base64ToBytes, looksLikeText } from '../chat/textAttachments'

const CONTENT = ['# Title', '', 'first line', 'second line', 'third line', ''].join('\n')

function decode(base64: string): string {
  return new TextDecoder().decode(base64ToBytes(base64))
}

describe('attachmentFromDocument', () => {
  it('slices the raw source lines, inclusive, and names the file', () => {
    const file = attachmentFromDocument({
      content: CONTENT,
      path: 'docs/guide.md',
      root: '/tmp/ws',
      startLine: 3,
      endLine: 5
    })
    expect(file.name).toBe('guide.md')
    expect(file.path).toBe('docs/guide.md')
    expect(file.absolutePath).toBe('/tmp/ws/docs/guide.md')
    expect(file.startLine).toBe(3)
    expect(file.endLine).toBe(5)
    expect(decode(file.contentBase64)).toBe('first line\nsecond line\nthird line')
    expect(file.size).toBe(decode(file.contentBase64).length)
    expect(looksLikeText(base64ToBytes(file.contentBase64))).toBe(true)
  })

  it('attaches a single line', () => {
    const file = attachmentFromDocument({
      content: CONTENT,
      path: 'docs/guide.md',
      root: '/tmp/ws',
      startLine: 3,
      endLine: 3
    })
    expect(file.startLine).toBe(3)
    expect(file.endLine).toBe(3)
    expect(decode(file.contentBase64)).toBe('first line')
  })

  it('drops the absolute path when the root is unknown', () => {
    const file = attachmentFromDocument({
      content: CONTENT,
      path: 'docs/guide.md',
      root: null,
      startLine: 1,
      endLine: 1
    })
    expect(file.absolutePath).toBeUndefined()
    expect(decode(file.contentBase64)).toBe('# Title')
  })

  it('normalizes a trailing slash on the root', () => {
    const file = attachmentFromDocument({
      content: CONTENT,
      path: 'docs/guide.md',
      root: '/tmp/ws/',
      startLine: 1,
      endLine: 1
    })
    expect(file.absolutePath).toBe('/tmp/ws/docs/guide.md')
  })

  it('refuses a slice over the per-file cap', () => {
    const big = Array.from({ length: 40000 }, () => 'x'.repeat(2)).join('\n')
    expect(() =>
      attachmentFromDocument({
        content: big,
        path: 'big.md',
        root: null,
        startLine: 1,
        endLine: 40000
      })
    ).toThrow(/larger than/)
  })
})
