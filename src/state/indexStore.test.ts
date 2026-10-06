import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../api/codeIndex', () => ({
  getIndexStatus: vi.fn(async () => ({ state: 'indexing', files_done: 0, files_total: 0 })),
  setIndexState: vi.fn(async () => ({ state: 'paused', files_done: 0, files_total: 0 }))
}))

import { index, patchIndex } from './indexStore'
import { patchWorkspaces } from './workspaceStore'

const reset = () => {
  patchIndex({ workspaceId: null, status: null, pending: null })
  patchWorkspaces({ activeWorkspaceId: null })
}

describe('applyFrame', () => {
  beforeEach(reset)
  afterEach(reset)

  it('applies the frame status, including the terminal ready transition', () => {
    patchWorkspaces({ activeWorkspaceId: 'w1' })
    patchIndex({
      workspaceId: 'w1',
      status: { state: 'indexing', files_done: 521, files_total: 521 },
      pending: null
    })

    const applied = index.applyFrame('w1', {
      state: 'ready',
      files_done: 521,
      files_total: 521,
      error: null
    })

    expect(applied).toBe(true)
    expect(index.status?.state).toBe('ready')
  })

  it('clears a pending pause once the frame reports paused', () => {
    patchWorkspaces({ activeWorkspaceId: 'w1' })
    patchIndex({
      workspaceId: 'w1',
      status: { state: 'indexing', files_done: 1, files_total: 2 },
      pending: 'pause'
    })

    index.applyFrame('w1', { state: 'paused', files_done: 1, files_total: 2 })

    expect(index.pending).toBeNull()
  })

  it('rejects a frame for another workspace', () => {
    patchWorkspaces({ activeWorkspaceId: 'w1' })

    const applied = index.applyFrame('w2', { state: 'ready' })

    expect(applied).toBe(false)
    expect(index.status).toBeNull()
  })

  it('rejects malformed frame data', () => {
    patchWorkspaces({ activeWorkspaceId: 'w1' })

    expect(index.applyFrame('w1', { state: 'indexing' })).toBe(false)
    expect(index.applyFrame('w1', null)).toBe(false)
  })
})
