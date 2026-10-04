import { describe, expect, it } from 'vitest'

import { noticePlacement } from './errorLog'

describe('noticePlacement', () => {
  it('pins the current session to the transcript', () => {
    expect(noticePlacement({ open: true, sessionId: 'a' }, 'a', true)).toBe('transcript')
  })

  it('puts another session and app errors at the top', () => {
    expect(noticePlacement({ open: true, sessionId: 'b' }, 'a', true)).toBe('top')
    expect(noticePlacement({ open: true, sessionId: null }, 'a', true)).toBe('top')
  })

  it('puts the current session at the top when the transcript is not on screen', () => {
    expect(noticePlacement({ open: true, sessionId: 'a' }, 'a', false)).toBe('top')
  })

  it('hides an acknowledged error', () => {
    expect(noticePlacement({ open: false, sessionId: 'a' }, 'a', true)).toBe('hidden')
  })
})
