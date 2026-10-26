import { createSignal } from 'solid-js'

import { apiBaseUrl } from '../../api/client'

const POLL_MS = 5_000
const TIMEOUT_MS = 3_000

const [reason, setReason] = createSignal<string | null>(null)

/** Why the last health check failed, or null while the API answers. */
export function apiHealthReason(): string | null {
  return reason()
}

let watchers = 0
let timer: number | undefined

/**
 * Watch the local API's health. Each caller gets its own stop function, and the
 * poll runs once however many callers there are: the top bar's overflow and the
 * legacy header both show the same mark.
 */
export function watchApiHealth(): () => void {
  watchers += 1
  if (watchers === 1) {
    void check()
    timer = window.setInterval(() => void check(), POLL_MS)
  }
  let stopped = false
  return () => {
    if (stopped) {
      return
    }
    stopped = true
    watchers -= 1
    if (watchers === 0 && timer !== undefined) {
      window.clearInterval(timer)
      timer = undefined
    }
  }
}

async function check(): Promise<void> {
  const controller = new AbortController()
  const timeout = window.setTimeout(() => controller.abort(), TIMEOUT_MS)
  const base = apiBaseUrl()
  const url = base ? `${base}/api/v1/health` : '/api/v1/health'
  try {
    const response = await fetch(url, { signal: controller.signal })
    setReason(failureReason(undefined, response, controller.signal.aborted))
  } catch (error) {
    setReason(failureReason(error, undefined, controller.signal.aborted))
  } finally {
    window.clearTimeout(timeout)
  }
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
