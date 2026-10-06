import { describe, expect, it, vi } from 'vitest'

import {
  requestComposerAttachment,
  subscribeComposerAttachments,
  takeComposerAttachments
} from './composerAttachments'
import type { FileAttachment } from '../features/chat/textAttachments'

function file(name: string): FileAttachment {
  return { name, contentBase64: '', size: 0 }
}

describe('composerAttachments', () => {
  it('takes a queued attachment once', () => {
    requestComposerAttachment('session-a', file('a.md'))
    expect(takeComposerAttachments('session-a').map((item) => item.name)).toEqual(['a.md'])
    expect(takeComposerAttachments('session-a')).toEqual([])
  })

  it('keeps queues separate per key', () => {
    requestComposerAttachment('session-a', file('a.md'))
    requestComposerAttachment('draft', file('b.md'))
    requestComposerAttachment('draft', file('c.md'))
    expect(takeComposerAttachments('draft').map((item) => item.name)).toEqual(['b.md', 'c.md'])
    expect(takeComposerAttachments('session-a').map((item) => item.name)).toEqual(['a.md'])
  })

  it('notifies subscribers on each request', () => {
    const listener = vi.fn()
    const unsubscribe = subscribeComposerAttachments(listener)
    requestComposerAttachment('session-a', file('a.md'))
    expect(listener).toHaveBeenCalledTimes(1)
    unsubscribe()
    requestComposerAttachment('session-a', file('b.md'))
    expect(listener).toHaveBeenCalledTimes(1)
  })
})
