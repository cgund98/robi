/** @jsxImportSource solid-js */
import { createSignal, onCleanup, onMount, Show } from 'solid-js'

import { apiBaseUrl } from '../../../api/client'
import styles from '../../../components/layout/ApiStatus.module.css'

const POLL_MS = 5_000
const TIMEOUT_MS = 3_000

export function ApiStatus() {
  const [reason, setReason] = createSignal<string | null>(null)

  onMount(() => {
    let cancelled = false

    const check = async () => {
      const controller = new AbortController()
      const timer = window.setTimeout(() => controller.abort(), TIMEOUT_MS)
      const base = apiBaseUrl()
      const url = base ? `${base}/api/v1/health` : '/api/v1/health'
      try {
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
    onCleanup(() => {
      cancelled = true
      window.clearInterval(id)
    })
  })

  return (
    <Show when={reason()}>
      {(message) => (
        <span class={styles.mark} role="status" title={message()} aria-label={message()}>
          !
        </span>
      )}
    </Show>
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
