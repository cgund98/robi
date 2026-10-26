import { describe, expect, it } from 'vitest'

import { showSurface } from './surfaces'

describe('showSurface', () => {
  it('turns off the surfaces the streamlining roadmap removes from the UI', () => {
    expect(showSurface('docsViewer')).toBe(false)
    expect(showSurface('reviewScreen')).toBe(false)
  })

  it('turns on the top bar shell', () => {
    expect(showSurface('topBarShell')).toBe(true)
  })
})
