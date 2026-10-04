import { useEffect, useState } from 'react'

import { listDocs, type DocEntry } from '../api/docs'
import { readListing, writeListing } from './docsCache'

/**
 * The markdown listing for a workspace, cached across visits.
 *
 * The first visit for a workspace shows the loading state. A later visit starts
 * from the cached listing — no spinner — and refreshes in the background, so the
 * tree only changes when the new listing arrives.
 */
export function useWorkspaceDocs(workspaceId: string | null): {
  files: DocEntry[]
  loading: boolean
  error: string | null
} {
  const cached = workspaceId ? (readListing(workspaceId) ?? null) : null
  const [files, setFiles] = useState<DocEntry[]>(cached ?? [])
  const [loading, setLoading] = useState(workspaceId !== null && cached === null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!workspaceId) {
      return
    }
    let cancelled = false
    void listDocs(workspaceId)
      .then((listing) => {
        if (cancelled) {
          return
        }
        writeListing(workspaceId, listing.files)
        setFiles(listing.files)
        setError(null)
      })
      .catch((err: unknown) => {
        if (cancelled || readListing(workspaceId)) {
          return
        }
        setError(err instanceof Error ? err.message : 'Failed to load docs')
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [workspaceId])

  return { files, loading, error }
}
