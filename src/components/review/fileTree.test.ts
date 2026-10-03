import { describe, expect, it } from 'vitest'

import { buildFileTree, filesInTreeOrder } from './tree'

describe('buildFileTree', () => {
  it('groups directories before files and sorts each group', () => {
    const paths = ['src/main.rs', 'README.md', 'docs/a.md', 'src/lib.rs', 'src/tools/edit.rs']
    const tree = buildFileTree(paths)
    expect(tree.map((node) => node.name)).toEqual(['docs', 'src', 'README.md'])
    const src = tree[1]
    expect(src.children.map((node) => `${node.kind}:${node.name}`)).toEqual([
      'dir:tools',
      'file:lib.rs',
      'file:main.rs'
    ])
    expect(filesInTreeOrder(paths)).toEqual([
      'docs/a.md',
      'src/tools/edit.rs',
      'src/lib.rs',
      'src/main.rs',
      'README.md'
    ])
  })

  it('sorts names without regard to case', () => {
    expect(buildFileTree(['b.ts', 'A.ts']).map((node) => node.name)).toEqual(['A.ts', 'b.ts'])
    expect(buildFileTree(['b/a.ts', 'A/a.ts']).map((node) => node.name)).toEqual(['A', 'b'])
  })
})
