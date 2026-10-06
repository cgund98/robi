import { createEffect, createSignal, onCleanup } from 'solid-js'

import { getSessionReview, type ReviewFileSummary } from '../../api/review'
import { chat } from '../state/host'

export function useSessionReview(sessionId: () => string): {
  files: () => ReviewFileSummary[]
  error: () => string | null
  loading: () => boolean
} {
  const [files, setFiles] = createSignal<ReviewFileSummary[]>([])
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(true)

  createEffect(() => {
    const id = sessionId()
    void (chat.reviewTickBySession[id] ?? 0)
    let cancelled = false
    setLoading(true)
    void getSessionReview(id)
      .then((review) => {
        if (!cancelled) {
          setFiles(review.files)
          setError(null)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load review')
        }
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

  return { files, error, loading }
}
