import { useEffect, useState } from 'react'

import { apiBaseUrl } from '../../api/client'
import styles from './ApiStatus.module.css'

const POLL_MS = 5_000
const TIMEOUT_MS = 3_000

export function ApiStatus() {
  const [reason, setReason] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    const check = async () => {
      const controller = new AbortController()
      const timer = window.setTimeout(() => controller.abort(), TIMEOUT_MS)
      const base = apiBaseUrl()
      const url = base ? `${base}/api/v1/health` : '/api/v1/health'
      try {
        // Direct fetch. The typed client builds a Request that already owns
        // this signal, and WebKit rejects a second fetch with that signal.
        const response = await fetch(url, { signal: controller.signal })
        if (!cancelled) {
          setReason(failureReason(undefined, response, controller.signal.aborted))
        }
      } catch (error) {
        if (!cancelled) {
          setReason(failureReason(error, undefined, controller.signal.aborted))
        }
      } finally {
        window.clearTimeout(timer)
      }
    }

    void check()
    const id = window.setInterval(() => void check(), POLL_MS)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [])

  if (!reason) {
    return null
  }

  return (
    <span className={styles.mark} role="status" title={reason} aria-label={reason}>
      !
    </span>
  )
}

function failureReason(
  error: unknown,
  response: Response | undefined,
  timedOut: boolean
): string | null {
  if (response?.ok && !error) {
    return null
  }
  if (timedOut || isTimeout(error)) {
    return 'Health check timed out after 3s'
  }
  if (response && !response.ok) {
    return `Health check failed: HTTP ${response.status}`
  }
  const detail = errorDetail(error)
  if (detail) {
    return `Health check failed: ${detail}`
  }
  return 'Health check failed'
}

function isTimeout(error: unknown): boolean {
  if (!error || typeof error !== 'object' || !('name' in error)) {
    return false
  }
  const name = String(error.name)
  return name === 'TimeoutError' || name === 'AbortError'
}

function errorDetail(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message
  }
  if (error && typeof error === 'object' && 'message' in error) {
    const message = error.message
    if (typeof message === 'string' && message) {
      return message
    }
  }
  return ''
}
