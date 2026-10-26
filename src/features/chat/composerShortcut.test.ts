import { describe, expect, it } from 'vitest'

import { clearComposerShortcut } from './composerShortcut'

type Event = Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey' | 'repeat'>

function key(overrides: Partial<Event>): Event {
  return {
    key: 'k',
    metaKey: false,
    ctrlKey: false,
    shiftKey: true,
    altKey: false,
    repeat: false,
    ...overrides
  }
}

describe('clearComposerShortcut', () => {
  it('matches the chord with meta or control', () => {
    expect(clearComposerShortcut(key({ metaKey: true }))).toBe(true)
    expect(clearComposerShortcut(key({ ctrlKey: true }))).toBe(true)
  })

  it('accepts an uppercase K', () => {
    expect(clearComposerShortcut(key({ key: 'K', metaKey: true }))).toBe(true)
  })

  it('needs the modifier and shift', () => {
    expect(clearComposerShortcut(key({}))).toBe(false)
    expect(clearComposerShortcut(key({ metaKey: true, shiftKey: false }))).toBe(false)
    expect(clearComposerShortcut(key({ shiftKey: false }))).toBe(false)
  })

  it('rejects alt, a held key, and another key', () => {
    expect(clearComposerShortcut(key({ metaKey: true, altKey: true }))).toBe(false)
    expect(clearComposerShortcut(key({ metaKey: true, repeat: true }))).toBe(false)
    expect(clearComposerShortcut(key({ key: 'j', metaKey: true }))).toBe(false)
  })
})
