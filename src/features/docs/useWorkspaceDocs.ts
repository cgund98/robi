import { createEffect, createSignal, onCleanup, untrack } from 'solid-js'

import { listDocs, type DocEntry } from '../../api/docs'
import { readExpanded, readListing, recordExpanded, writeListing } from './docsCache'
import { markFetched, mergeLevel, mergeRecursive, sameEntries } from './docsTree'

/** The markdown listing for a workspace, cached across visits. */
export function useWorkspaceDocs(
  workspaceId: () => string | null,
  tick: () => number
): {
  entries: () => DocEntry[]
  loading: () => boolean
  error: () => string | null
  loadDirectory: (path: string) => void
} {
  const initial = workspaceId()
  const cached = initial ? (readListing(initial) ?? null) : null
  const [entries, setEntries] = createSignal<DocEntry[]>(cached ?? [])
  const [loading, setLoading] = createSignal(initial !== null && cached === null)
  const [error, setError] = createSignal<string | null>(null)
  /**
   * Directories the user expanded. Stored with the viewer so leaving docs and
   * coming back does not fold them when the root listing arrives.
   */
  const expanded = new Set<string>(initial ? readExpanded(initial) : [])
  const levelToken = new Map<string, number>()

  function apply(id: string, produce: (current: DocEntry[]) => DocEntry[]) {
    setEntries((current) => {
      const next = produce(current)
      if (sameEntries(current, next)) {
        return current
      }
      writeListing(id, next)
      return next
    })
  }

  function loadDirectory(path: string) {
    expanded.add(path)
    const id = untrack(workspaceId)
    if (id) {
      recordExpanded(id, expanded)
    }
    if (!id) {
      return
    }
    const token = (levelToken.get(path) ?? 0) + 1
    levelToken.set(path, token)
    void listDocs(id, { path, recursive: false })
      .then((listing) => {
        if (levelToken.get(path) !== token) {
          return
        }
        apply(id, (current) =>
          markFetched(mergeLevel(current, listing.path, listing.entries, expanded), listing.path)
        )
        setError(null)
      })
      .catch(() => {
        // A failed refresh keeps the cached children.
      })
  }

  createEffect(() => {
    const id = workspaceId()
    tick()
    if (!id) {
      setEntries([])
      setLoading(false)
      return
    }
    const stored = readListing(id)
    if (stored && untrack(entries).length === 0) {
      setEntries(stored)
      setLoading(false)
    }
    let cancelled = false
    const expandedNow = [...expanded]
    void listDocs(id, { recursive: true })
      .then((listing) => {
        if (cancelled) {
          return
        }
        apply(id, (current) => mergeRecursive(current, listing.entries, expanded))
        setError(null)
      })
      .catch((err: unknown) => {
        if (cancelled || readListing(id)) {
          return
        }
        setError(err instanceof Error ? err.message : 'Failed to load docs')
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
        }
      })
    for (const path of expandedNow) {
      const token = (levelToken.get(path) ?? 0) + 1
      levelToken.set(path, token)
      void listDocs(id, { path, recursive: false })
        .then((listing) => {
          if (cancelled || levelToken.get(path) !== token) {
            return
          }
          apply(id, (current) =>
            markFetched(mergeLevel(current, listing.path, listing.entries, expanded), listing.path)
          )
        })
        .catch(() => {})
    }
    onCleanup(() => {
      cancelled = true
    })
  })

  return { entries, loading, error, loadDirectory }
}
