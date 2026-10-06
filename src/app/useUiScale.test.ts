import { describe, expect, it } from 'vitest'

import { scaleShortcut } from './useUiScale'

type Event = Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey'>

function key(overrides: Partial<Event>): Event {
  return { key: '', metaKey: false, ctrlKey: false, altKey: false, ...overrides }
}

describe('scaleShortcut', () => {
  it('maps the plus, minus, and zero keys', () => {
    expect(scaleShortcut(key({ key: '+', metaKey: true }), true)).toBe('in')
    expect(scaleShortcut(key({ key: '=', metaKey: true }), true)).toBe('in')
    expect(scaleShortcut(key({ key: '-', metaKey: true }), true)).toBe('out')
    expect(scaleShortcut(key({ key: '_', metaKey: true }), true)).toBe('out')
    expect(scaleShortcut(key({ key: '0', metaKey: true }), true)).toBe('reset')
  })

  it('uses meta on mac and control elsewhere', () => {
    expect(scaleShortcut(key({ key: '+', metaKey: true }), true)).toBe('in')
    expect(scaleShortcut(key({ key: '+', ctrlKey: true }), true)).toBeNull()
    expect(scaleShortcut(key({ key: '+', ctrlKey: true }), false)).toBe('in')
    expect(scaleShortcut(key({ key: '+', metaKey: true }), false)).toBeNull()
  })

  it('bails without the modifier, with alt, or on another key', () => {
    expect(scaleShortcut(key({ key: '+' }), true)).toBeNull()
    expect(scaleShortcut(key({ key: '+', metaKey: true, altKey: true }), true)).toBeNull()
    expect(scaleShortcut(key({ key: 'a', metaKey: true }), true)).toBeNull()
  })
})
