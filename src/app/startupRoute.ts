/**
 * Docs opens off on a fresh launch.
 *
 * The docs viewer is a route (`#/docs`), so a hash retained from the last
 * session would reopen it on the next load. This clears that on startup only;
 * the sidebar toggle still navigates to `#/docs` during a session.
 */
export function withoutDocsRoute(hash: string): string {
  return hash === '#/docs' || hash.startsWith('#/docs?') || hash.startsWith('#/docs/') ? '#/' : hash
}
