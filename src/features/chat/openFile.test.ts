import { describe, expect, it } from 'vitest'

import { matchesOpenPattern } from './openFile'

describe('matchesOpenPattern', () => {
  it('matches a glob against the whole relative path', () => {
    expect(matchesOpenPattern('src/main.rs', '*.rs\n*.png')).toBe(true)
    expect(matchesOpenPattern('docs/a.md', '*.rs\n*.png')).toBe(false)
  })

  it('treats other characters as literal', () => {
    expect(matchesOpenPattern('file.txt', 'file.txt')).toBe(true)
    expect(matchesOpenPattern('file.txt.bak', 'file.txt')).toBe(false)
  })

  it('matches nothing when the list is empty', () => {
    expect(matchesOpenPattern('docs/a.md', '')).toBe(false)
    expect(matchesOpenPattern('docs/a.md', '\n  \n')).toBe(false)
  })
})