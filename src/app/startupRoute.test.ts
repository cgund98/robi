import { describe, expect, it } from 'vitest'

import { withoutDocsRoute } from './startupRoute'

describe('withoutDocsRoute', () => {
  it('leaves every non-docs hash alone', () => {
    expect(withoutDocsRoute('')).toBe('')
    expect(withoutDocsRoute('#/')).toBe('#/')
    expect(withoutDocsRoute('#/settings/providers')).toBe('#/settings/providers')
    expect(withoutDocsRoute('#/sessions/abc/review')).toBe('#/sessions/abc/review')
  })

  it('sends a retained docs hash to the chat route', () => {
    expect(withoutDocsRoute('#/docs')).toBe('#/')
    expect(withoutDocsRoute('#/docs?file=docs/a.md')).toBe('#/')
    expect(withoutDocsRoute('#/docs/anything')).toBe('#/')
  })

  it('does not catch a path that merely starts with docs', () => {
    expect(withoutDocsRoute('#/documentation')).toBe('#/documentation')
  })
})
