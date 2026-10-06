/** @jsxImportSource solid-js */
import { useNavigate } from '@solidjs/router'
import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

import { getSessionReview, type ReviewFileSummary } from '../../../api/review'
import styles from '../../../components/chat/EditReviewStrip.module.css'
import { chat } from '../../state/host'

export function EditReviewStrip(props: { sessionId: string }) {
  const navigate = useNavigate()
  const [files, setFiles] = createSignal<ReviewFileSummary[]>([])

  createEffect(() => {
    const sessionId = props.sessionId
    void (chat.reviewTickBySession[sessionId] ?? 0)
    let cancelled = false
    void getSessionReview(sessionId)
      .then((review) => {
        if (!cancelled) {
          setFiles(review.files)
        }
      })
      .catch(() => {
        if (!cancelled) {
          setFiles([])
        }
      })
    onCleanup(() => {
      cancelled = true
    })
  })

  return (
    <Show when={files().length > 0}>
      <div class={styles.strip}>
        <div class={styles.inner}>
          <span>
            {files().length} file{files().length === 1 ? '' : 's'} edited
          </span>
          <button
            type="button"
            class={styles.review}
            onClick={() => navigate(`/sessions/${props.sessionId}/review`)}
          >
            Review
          </button>
        </div>
      </div>
    </Show>
  )
}
