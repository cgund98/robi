import { describe, expect, it } from 'vitest'

import {
  buildDocTree,
  markFetched,
  mergeLevel,
  mergeRecursive,
  type DocTreeEntry
} from './docsTree'

const readme: DocTreeEntry = { path: 'README.md', kind: 'file', ignored: false }
const docsDir: DocTreeEntry = {
  path: 'docs',
  kind: 'directory',
  ignored: false,
  children_fetched: true
}
const page: DocTreeEntry = { path: 'docs/a.md', kind: 'file', ignored: false }
const scratch: DocTreeEntry = {
  path: 'scratch',
  kind: 'directory',
  ignored: true,
  children_fetched: false
}

describe('mergeLevel', () => {
  it('keeps grandchildren of a directory the one-level response did not fetch', () => {
    const current: DocTreeEntry[] = [
      scratch,
      { path: 'scratch/note.md', kind: 'file', ignored: true },
      { path: 'scratch/nested', kind: 'directory', ignored: true, children_fetched: true },
      { path: 'scratch/nested/deep.md', kind: 'file', ignored: true },
      { path: 'scratch/gone.md', kind: 'file', ignored: true }
    ]
    const merged = mergeLevel(current, 'scratch', [
      { path: 'scratch/note.md', kind: 'file', ignored: true },
      { path: 'scratch/nested', kind: 'directory', ignored: true, children_fetched: false }
    ])
    const paths = merged.map((entry) => entry.path).sort()
    expect(paths).toEqual([
      'scratch',
      'scratch/nested',
      'scratch/nested/deep.md',
      'scratch/note.md'
    ])
  })
})

describe('mergeRecursive', () => {
  it('keeps an expanded ignored directory open when the root listing still contains it', () => {
    const current: DocTreeEntry[] = [
      readme,
      scratch,
      { path: 'scratch/note.md', kind: 'file', ignored: true }
    ]
    const merged = mergeRecursive(current, [readme, page, docsDir, scratch], new Set(['scratch']))
    expect(merged.find((entry) => entry.path === 'scratch')?.children_fetched).toBe(true)
    expect(merged.some((entry) => entry.path === 'scratch/note.md')).toBe(true)
    expect(merged.some((entry) => entry.path === 'docs/a.md')).toBe(true)
  })

  it('drops a fetched directory the root listing no longer contains', () => {
    const current: DocTreeEntry[] = [
      readme,
      { ...scratch, children_fetched: true },
      { path: 'scratch/note.md', kind: 'file', ignored: true }
    ]
    const merged = mergeRecursive(current, [readme, page, docsDir], new Set(['scratch']))
    expect(
      merged.some((entry) => entry.path === 'scratch' || entry.path.startsWith('scratch/'))
    ).toBe(false)
  })
})

describe('markFetched', () => {
  it('records that a directory listing arrived', () => {
    const marked = markFetched([scratch], 'scratch')
    expect(marked[0]?.children_fetched).toBe(true)
  })
})

describe('buildDocTree', () => {
  it('nests an unfetched ignored directory beside fetched pages', () => {
    const tree = buildDocTree([readme, docsDir, page, scratch])
    const ignored = tree.find((node) => node.path === 'scratch')
    expect(ignored?.ignored).toBe(true)
    expect(ignored?.childrenFetched).toBe(false)
    expect(ignored?.children).toEqual([])
    const docs = tree.find((node) => node.path === 'docs')
    expect(docs?.childrenFetched).toBe(true)
    expect(docs?.children.map((node) => node.path)).toEqual(['docs/a.md'])
  })
})
