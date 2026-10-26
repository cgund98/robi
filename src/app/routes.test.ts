import { describe, expect, it } from 'vitest'

import { chatRoutePaths } from './routes'
import type { UiSurface } from './surfaces'

function showing(...on: UiSurface[]): (surface: UiSurface) => boolean {
  return (surface) => on.includes(surface)
}

describe('chatRoutePaths', () => {
  it('always serves the chat and a session route', () => {
    expect(chatRoutePaths(showing())).toEqual(['/', '/sessions/:sessionId'])
  })

  it('adds the docs route only while the viewer is on', () => {
    expect(chatRoutePaths(showing('docsViewer'))).toContain('/docs')
    expect(chatRoutePaths(showing())).not.toContain('/docs')
  })

  it('adds the review route only while the review screen is on', () => {
    expect(chatRoutePaths(showing('reviewScreen'))).toContain('/sessions/:sessionId/review')
    expect(chatRoutePaths(showing())).not.toContain('/sessions/:sessionId/review')
  })

  it('serves only the chat routes with every removed surface off', () => {
    const paths = chatRoutePaths(showing('topBarShell'))
    expect(paths).toEqual(['/', '/sessions/:sessionId'])
  })
})
