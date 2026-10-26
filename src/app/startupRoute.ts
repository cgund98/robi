/**
 * Docs opens off on a fresh launch, and the viewer has no route at all while S5
 * is in effect.
 *
 * The docs viewer was a route (`#/docs`), so a hash retained from the last
 * session would reopen it on the next load. With the route gone it would match
 * nothing, so this is now the only thing that keeps an old hash out of an empty
 * page.
 */
export function withoutDocsRoute(hash: string): string {
  return hash === '#/docs' || hash.startsWith('#/docs?') || hash.startsWith('#/docs/') ? '#/' : hash
}
