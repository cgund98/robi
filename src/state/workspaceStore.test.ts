import { describe, expect, it } from 'vitest'

import type { Workspace } from '../api/workspaces'
import { orderByRecentUse } from './workspaceStore'

function workspace(id: string): Workspace {
  return { id, name: id, root: `/${id}`, created_at: '' }
}

describe('orderByRecentUse', () => {
  it('puts the most recently selected workspace first and keeps the rest in list order', () => {
    const list = [workspace('a'), workspace('b'), workspace('c')]
    expect(orderByRecentUse(list, ['c', 'a']).map((item) => item.id)).toEqual(['c', 'a', 'b'])
  })

  it('leaves the list alone when nothing has been selected', () => {
    const list = [workspace('a'), workspace('b')]
    expect(orderByRecentUse(list, []).map((item) => item.id)).toEqual(['a', 'b'])
  })
})
