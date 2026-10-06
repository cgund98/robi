import { ChangeSet } from '@codemirror/state'
import { createRoot } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { putDoc } from '../../api/docs'
import { serializeChangeSet } from './docsEditor'
import { AUTOSAVE_DELAY_MS, useDocAutosave } from './useDocAutosave'

vi.mock('../../api/docs', () => ({ putDoc: vi.fn() }))

const putDocMock = vi.mocked(putDoc)

function saved(content: string, version: string, outcome: string) {
  return {
    kind: 'saved' as const,
    doc: { path: 'docs/a.md', content, version, outcome }
  }
}

/** Mount the hook in a root so `onCleanup` has an owner. */
function mount(readText: () => string, onRemoteText: (text: string) => void = () => {}) {
  let hook!: ReturnType<typeof useDocAutosave>
  const dispose = createRoot((dispose) => {
    hook = useDocAutosave({
      workspaceId: () => 'ws',
      path: () => 'docs/a.md',
      sessionId: () => null,
      readText,
      onRemoteText
    })
    return dispose
  })
  return { hook, dispose }
}

const insert = (from: number, text: string, docLength: number) =>
  ChangeSet.of({ from, to: from, insert: text }, docLength)

describe('serializeChangeSet', () => {
  it('reports base offsets and the inserted text', () => {
    const set = ChangeSet.of({ from: 0, to: 1, insert: 'x' }, 3)
    expect(serializeChangeSet(set)).toEqual([{ from: 0, to: 1, insert: 'x' }])
  })

  it('is empty for a no-op set', () => {
    expect(serializeChangeSet(ChangeSet.empty(4))).toEqual([])
  })
})

describe('useDocAutosave', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    putDocMock.mockReset()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('saves one debounce after the last change', async () => {
    putDocMock.mockResolvedValue(saved('hello!', 'v2', 'applied'))
    const { hook } = mount(() => 'hello')

    hook.noteChange(insert(5, '!', 5))
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS - 1)
    expect(putDocMock).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(putDocMock).toHaveBeenCalledTimes(1)
    expect(putDocMock.mock.calls[0][2]).toEqual({
      session_id: null,
      base_version: null,
      changes: [{ from: 5, to: 5, insert: '!' }]
    })

    await vi.advanceTimersByTimeAsync(0)
    expect(hook.dirty()).toBe(false)
    expect(hook.status()).toEqual({ kind: 'saved', outcome: 'applied' })
  })

  it('resets the timer on each keystroke', async () => {
    putDocMock.mockResolvedValue(saved('ab', 'v2', 'applied'))
    const { hook } = mount(() => 'ab')

    hook.noteChange(insert(0, 'a', 0))
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS - 200)
    hook.noteChange(insert(1, 'b', 1))
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS - 200)
    expect(putDocMock).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(200)
    expect(putDocMock).toHaveBeenCalledTimes(1)
  })

  it('keeps input that arrives during a flight for the next save', async () => {
    let release!: (value: ReturnType<typeof saved>) => void
    putDocMock.mockReturnValueOnce(
      new Promise((resolve) => {
        release = resolve
      })
    )
    const { hook } = mount(() => 'ac')

    hook.noteChange(insert(1, 'a', 1))
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS)
    expect(putDocMock).toHaveBeenCalledTimes(1)

    // Typed while the first save is still open.
    hook.noteChange(insert(2, 'c', 2))
    release(saved('ac', 'v2', 'applied'))
    await vi.advanceTimersByTimeAsync(0)

    expect(hook.dirty()).toBe(true)
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS)
    expect(putDocMock).toHaveBeenCalledTimes(2)
    expect(putDocMock.mock.calls[1][2]).toEqual({
      session_id: null,
      base_version: 'v2',
      changes: [{ from: 2, to: 2, insert: 'c' }]
    })
  })

  it('re-sends the whole buffer after a 409', async () => {
    putDocMock
      .mockResolvedValueOnce({ kind: 'conflict', content: 'disk text', version: 'vd' })
      .mockResolvedValueOnce(saved('the buffer', 'vn', 'applied'))
    const { hook } = mount(() => 'the buffer')

    hook.noteChange(insert(0, 'x', 0))
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS)
    expect(putDocMock.mock.calls[0][2]).toMatchObject({ base_version: null })

    await vi.advanceTimersByTimeAsync(1)
    expect(putDocMock).toHaveBeenCalledTimes(2)
    const body = putDocMock.mock.calls[1][2]
    expect(body.content).toBe('the buffer')
    expect(body.base_version).toBe('vd')
    expect(body.changes).toBeUndefined()

    await vi.advanceTimersByTimeAsync(1)
    expect(hook.dirty()).toBe(false)
  })

  it('adopts the merged text when a save folded in an agent edit', async () => {
    const remote: string[] = []
    putDocMock.mockResolvedValue(saved('merged text', 'v3', 'merged'))
    const { hook } = mount(
      () => 'my text',
      (text) => remote.push(text)
    )

    hook.noteChange(insert(0, 'y', 0))
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS)
    await vi.advanceTimersByTimeAsync(0)

    expect(remote).toEqual(['merged text'])
    expect(hook.status()).toEqual({ kind: 'saved', outcome: 'merged' })
  })

  it('keeps the pending change and backs off after an error', async () => {
    putDocMock
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce(saved('ab', 'v2', 'applied'))
    const { hook } = mount(() => 'ab')

    hook.noteChange(insert(0, 'a', 0))
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS)
    await vi.advanceTimersByTimeAsync(0)

    expect(hook.dirty()).toBe(true)
    expect(hook.status().kind).toBe('error')

    // The first backoff is one debounce; the pending set was restored.
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS)
    expect(putDocMock).toHaveBeenCalledTimes(2)
    expect(putDocMock.mock.calls[1][2]).toMatchObject({
      changes: [{ from: 0, to: 0, insert: 'a' }]
    })
  })

  it('flush saves immediately and ignores the debounce', async () => {
    putDocMock.mockResolvedValue(saved('z', 'v2', 'applied'))
    const { hook } = mount(() => 'z')

    hook.noteChange(insert(0, 'z', 0))
    await hook.flush()
    expect(putDocMock).toHaveBeenCalledTimes(1)
    expect(hook.dirty()).toBe(false)
  })

  it('does nothing when there is nothing pending', async () => {
    const { hook } = mount(() => 'same')
    await hook.flush()
    expect(putDocMock).not.toHaveBeenCalled()
  })
})
