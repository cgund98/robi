/**
 * Which UI surfaces are shown.
 *
 * The streamlining roadmap removes a surface without removing its code: the
 * route, component, and store stay in the tree, and the render sites ask here.
 * Flipping a value back to `true` restores the surface.
 *
 * `topBarShell` is a layout flag only. It selects which chrome the chat shell
 * renders and which window bar tone `WindowFrame` paints; it never gates a
 * backend call.
 *
 * See `docs/src/streamlining.md`.
 */
export type UiSurface = 'docsViewer' | 'reviewScreen' | 'indexStatus' | 'topBarShell'

const SURFACES: Record<UiSurface, boolean> = {
  /** S5: the `#/docs` viewer, its chat tray, and the chat/docs toggle. */
  docsViewer: false,
  /** S2: the session review screen and the edited-file strip. */
  reviewScreen: false,
  /** S3, UI half: the index status pill. The index itself still runs. */
  indexStatus: false,
  /** S6: the top bar in place of the permanent left sidebar. */
  topBarShell: true
}

export function showSurface(surface: UiSurface): boolean {
  return SURFACES[surface]
}
