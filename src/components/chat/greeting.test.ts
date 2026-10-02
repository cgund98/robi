import { describe, expect, it } from 'vitest'

import { greetingLabel } from './greeting'

describe('greetingLabel', () => {
  it('follows the local hour', () => {
    expect(greetingLabel(new Date(2026, 9, 2, 8))).toBe('Good morning')
    expect(greetingLabel(new Date(2026, 9, 2, 13))).toBe('Good afternoon')
    expect(greetingLabel(new Date(2026, 9, 2, 19))).toBe('Good evening')
  })
})
