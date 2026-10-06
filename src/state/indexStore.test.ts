import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('../api/codeIndex', () => ({
  getIndexStatus: vi.fn(async () => ({ state: 'indexing', files_done: 0, files_total: 0 })),
  setIndexState: vi.fn(async () => ({ state: 'paused', files_done: 0, files_total: 0 }))
}))

import { useIndexStore } from './indexStore'
import { useWorkspaceStore } from './workspaceStore'

const reset = () => {
  useIndexStore.setState({ workspaceId: null, status: null, pending: null })
  useWorkspaceStore.setState({ activeWorkspaceId: null })
}

describe('applyFrame', () => {
  beforeEach(reset)
  afterEach(reset)

  it('applies the frame status, including the terminal ready transition', () => {
    useWorkspaceStore.setState({ activeWorkspaceId: 'w1' })
    useIndexStore.setState({
      workspaceId: 'w1',
      status: { state: 'indexing', files_done: 521, files_total: 521 },
      pending: null
    })

    const applied = useIndexStore
      .getState()
      .applyFrame('w1', { state: 'ready', files_done: 521, files_total: 521, error: null })

    expect(applied).toBe(true)
    expect(useIndexStore.getState().status?.state).toBe('ready')
  })

  it('clears a pending pause once the frame reports paused', () => {
    useWorkspaceStore.setState({ activeWorkspaceId: 'w1' })
    useIndexStore.setState({
      workspaceId: 'w1',
      status: { state: 'indexing', files_done: 1, files_total: 2 },
      pending: 'pause'
    })

    useIndexStore.getState().applyFrame('w1', { state: 'paused', files_done: 1, files_total: 2 })

    expect(useIndexStore.getState().pending).toBeNull()
  })

  it('rejects a frame for another workspace', () => {
    useWorkspaceStore.setState({ activeWorkspaceId: 'w1' })

    const applied = useIndexStore.getState().applyFrame('w2', { state: 'ready' })

    expect(applied).toBe(false)
    expect(useIndexStore.getState().status).toBeNull()
  })

  it('rejects malformed frame data', () => {
    useWorkspaceStore.setState({ activeWorkspaceId: 'w1' })

    expect(useIndexStore.getState().applyFrame('w1', { state: 'indexing' })).toBe(false)
    expect(useIndexStore.getState().applyFrame('w1', null)).toBe(false)
  })
})
