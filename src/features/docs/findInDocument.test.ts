import { describe, expect, it } from 'vitest'

import { collectMatches, matchRanges } from './findInDocument'

function rootWith(html: string): HTMLElement {
  const root = document.createElement('div')
  root.innerHTML = html
  return root
}

describe('matchRanges', () => {
  it('returns nothing for an empty query', () => {
    expect(matchRanges('hello', '', false)).toEqual([])
  })

  it('is case-insensitive by default', () => {
    expect(matchRanges('Hello hello', 'hello', false)).toEqual([
      { start: 0, end: 5 },
      { start: 6, end: 11 }
    ])
  })

  it('honours case sensitivity', () => {
    expect(matchRanges('Hello hello', 'hello', true)).toEqual([{ start: 6, end: 11 }])
  })

  it('does not overlap', () => {
    expect(matchRanges('aaaa', 'aa', false)).toEqual([
      { start: 0, end: 2 },
      { start: 2, end: 4 }
    ])
  })

  it('treats the query literally', () => {
    expect(matchRanges('a.b aXb', 'a.b', false)).toEqual([{ start: 0, end: 3 }])
  })
})

describe('collectMatches', () => {
  it('finds matches across text nodes in document order', () => {
    const ranges = collectMatches(rootWith('<p>alpha beta</p><p>alpha</p>'), 'alpha', false)
    expect(ranges.map((range) => range.toString())).toEqual(['alpha', 'alpha'])
  })

  it('skips svg, aria-hidden, and data-find-ignore subtrees', () => {
    const root = rootWith(
      '<p>hit</p>' +
        '<svg><text>hit</text></svg>' +
        '<span aria-hidden="true">hit</span>' +
        '<span data-find-ignore>hit</span>'
    )
    const ranges = collectMatches(root, 'hit', false)
    expect(ranges).toHaveLength(1)
  })

  it('honours case sensitivity', () => {
    const root = rootWith('<p>Hello hello</p>')
    expect(collectMatches(root, 'hello', true)).toHaveLength(1)
    expect(collectMatches(root, 'hello', false)).toHaveLength(2)
  })

  it('returns nothing for an empty query', () => {
    expect(collectMatches(rootWith('<p>text</p>'), '', false)).toEqual([])
  })
})
