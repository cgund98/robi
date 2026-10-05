import { describe, expect, it } from 'vitest'

import { noticePlacement } from './errorLog'

describe('noticePlacement', () => {
  it('puts every open error at the top of the shell', () => {
    expect(noticePlacement({ open: true })).toBe('top')
  })

  it('hides an acknowledged error', () => {
    expect(noticePlacement({ open: false })).toBe('hidden')
  })
})
