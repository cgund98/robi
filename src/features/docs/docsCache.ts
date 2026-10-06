import type { DocEntry } from '../../api/docs'

/**
 * Session-lifetime caches for the docs viewer.
 *
 * Module scope, so a return to the page repaints from the last visit before the
 * refresh lands. Cleared when the app reloads.
 */

const listings = new Map<string, DocEntry[]>()
const contents = new Map<string, string>()
const viewers = new Map<string, ViewerState>()

/** Per-workspace viewer memory: the open document and each document's scroll. */
type ViewerState = {
  path: string | null
  scroll: Record<string, number>
  collapsed: string[]
}

export function readListing(workspaceId: string): DocEntry[] | undefined {
  return listings.get(workspaceId)
}

export function writeListing(workspaceId: string, files: DocEntry[]): void {
  listings.set(workspaceId, files)
}

function contentKey(workspaceId: string, path: string): string {
  return `${workspaceId}\u0000${path}`
}

export function readContent(workspaceId: string, path: string): string | undefined {
  return contents.get(contentKey(workspaceId, path))
}

export function writeContent(workspaceId: string, path: string, text: string): void {
  contents.set(contentKey(workspaceId, path), text)
}

/** The last opened document, if it is still in the cached listing. */
export function lastValidPath(workspaceId: string | null): string | null {
  if (!workspaceId) {
    return null
  }
  const path = viewers.get(workspaceId)?.path ?? null
  if (!path) {
    return null
  }
  const listing = readListing(workspaceId)
  if (listing && !listing.some((file) => file.path === path)) {
    return null
  }
  return path
}

export function recordPath(workspaceId: string, path: string | null): void {
  const state = viewers.get(workspaceId) ?? { path: null, scroll: {}, collapsed: [] }
  viewers.set(workspaceId, { ...state, path })
}

export function readScroll(workspaceId: string, path: string): number {
  return viewers.get(workspaceId)?.scroll[path] ?? 0
}

export function recordScroll(workspaceId: string, path: string, offset: number): void {
  const state = viewers.get(workspaceId) ?? { path: null, scroll: {}, collapsed: [] }
  viewers.set(workspaceId, { ...state, scroll: { ...state.scroll, [path]: offset } })
}

/** Directory paths the user folded closed in this workspace. */
export function readCollapsed(workspaceId: string): string[] {
  return viewers.get(workspaceId)?.collapsed ?? []
}

export function recordCollapsed(workspaceId: string, collapsed: Iterable<string>): void {
  const state = viewers.get(workspaceId) ?? { path: null, scroll: {}, collapsed: [] }
  viewers.set(workspaceId, { ...state, collapsed: [...collapsed] })
}
