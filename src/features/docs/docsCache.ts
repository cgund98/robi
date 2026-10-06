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
  mode: DocViewMode
  versions: Record<string, string>
}

/** Preview or editing. The toolbar's two states. */
export type DocViewMode = 'rendered' | 'edit'

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
  viewers.set(workspaceId, { ...viewer(workspaceId), path })
}

export function readScroll(workspaceId: string, path: string): number {
  return viewers.get(workspaceId)?.scroll[path] ?? 0
}

export function recordScroll(workspaceId: string, path: string, offset: number): void {
  const state = viewer(workspaceId)
  viewers.set(workspaceId, { ...state, scroll: { ...state.scroll, [path]: offset } })
}

/** Directory paths the user folded closed in this workspace. */
export function readCollapsed(workspaceId: string): string[] {
  return viewers.get(workspaceId)?.collapsed ?? []
}

export function recordCollapsed(workspaceId: string, collapsed: Iterable<string>): void {
  const state = viewer(workspaceId)
  viewers.set(workspaceId, { ...state, collapsed: [...collapsed] })
}

/** The remembered Rendered|Edit choice for this workspace. Default rendered. */
export function readMode(workspaceId: string): DocViewMode {
  return viewers.get(workspaceId)?.mode ?? 'rendered'
}

export function recordMode(workspaceId: string, mode: DocViewMode): void {
  viewers.set(workspaceId, { ...viewer(workspaceId), mode })
}

/** The last version the server acked for this document, if any. */
export function readVersion(workspaceId: string, path: string): string | undefined {
  return viewers.get(workspaceId)?.versions[path]
}

export function writeVersion(workspaceId: string, path: string, version: string): void {
  const state = viewer(workspaceId)
  viewers.set(workspaceId, {
    ...state,
    versions: { ...state.versions, [path]: version }
  })
}

function viewer(workspaceId: string): ViewerState {
  return (
    viewers.get(workspaceId) ?? {
      path: null,
      scroll: {},
      collapsed: [],
      mode: 'rendered',
      versions: {}
    }
  )
}
