import { useNavigate } from 'react-router-dom'

import { useSessionReview } from '../../app/useSessionReview'
import styles from './EditReviewStrip.module.css'

type EditReviewStripProps = {
  sessionId: string
}

export function EditReviewStrip({ sessionId }: EditReviewStripProps) {
  const navigate = useNavigate()
  const { files } = useSessionReview(sessionId)
  if (files.length === 0) {
    return null
  }
  const count = files.length
  return (
    <div className={styles.strip}>
      <div className={styles.inner}>
        <span>
          {count} file{count === 1 ? '' : 's'} edited
        </span>
        <button
          type="button"
          className={styles.review}
          onClick={() => navigate(`/sessions/${sessionId}/review`)}
        >
          Review
        </button>
      </div>
    </div>
  )
}
