import type { UiSurface } from './surfaces'

/**
 * The routes the chat shell serves.
 *
 * A surface the streamlining roadmap removes drops out of this list, and
 * `app/SolidApp.tsx` decides what a retained hash does instead. The chat and a
 * session's own route always ship.
 */
export function chatRoutePaths(show: (surface: UiSurface) => boolean): string[] {
  const paths = ['/', '/sessions/:sessionId']
  if (show('docsViewer')) {
    paths.push('/docs')
  }
  if (show('reviewScreen')) {
    paths.push('/sessions/:sessionId/review')
  }
  return paths
}
