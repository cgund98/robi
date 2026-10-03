import { useEffect, useState } from 'react'

import { getSessionReview, type ReviewFile } from '../api/review'
import { useChatStore } from '../state/chatStore'

export function useSessionReview(sessionId: string): {
  files: ReviewFile[]
  error: string | null
  loading: boolean
} {
  const tick = useChatStore((state) => state.reviewTickBySession[sessionId] ?? 0)
  const [files, setFiles] = useState<ReviewFile[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    void getSessionReview(sessionId)
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
    return () => {
      cancelled = true
    }
  }, [sessionId, tick])

  return { files, error, loading }
}
