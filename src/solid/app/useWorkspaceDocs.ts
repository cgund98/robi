import { createEffect, createSignal, onCleanup } from 'solid-js'

import { listDocs, type DocEntry } from '../../api/docs'
import { readListing, writeListing } from '../../app/docsCache'

/** The markdown listing for a workspace, cached across visits. */
export function useWorkspaceDocs(
  workspaceId: () => string | null,
  tick: () => number
): {
  files: () => DocEntry[]
  loading: () => boolean
  error: () => string | null
} {
  const initial = workspaceId()
  const cached = initial ? (readListing(initial) ?? null) : null
  const [files, setFiles] = createSignal<DocEntry[]>(cached ?? [])
  const [loading, setLoading] = createSignal(initial !== null && cached === null)
  const [error, setError] = createSignal<string | null>(null)

  createEffect(() => {
    const id = workspaceId()
    tick()
    if (!id) {
      setFiles([])
      setLoading(false)
      return
    }
    const stored = readListing(id)
    if (stored && files().length === 0) {
      setFiles(stored)
      setLoading(false)
    }
    let cancelled = false
    void listDocs(id)
      .then((listing) => {
        if (cancelled) {
          return
        }
        writeListing(id, listing.files)
        setFiles(listing.files)
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
    onCleanup(() => {
      cancelled = true
    })
  })

  return { files, loading, error }
}
