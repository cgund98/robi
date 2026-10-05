import type { Middleware } from 'openapi-fetch'

import { useErrorLog } from '../state/errorLog'
import { useRequestLog } from '../state/requestLog'

const HEALTH_PATH = '/api/v1/health'

const startedAt = new Map<string, number>()

function noteStart(id: string): void {
  startedAt.set(id, performance.now())
}

function takeDuration(id: string): number {
  const start = startedAt.get(id)
  startedAt.delete(id)
  if (start === undefined) {
    return 0
  }
  return Math.round(performance.now() - start)
}

const MAX_BODY = 32_000

function recordRequest(
  id: string,
  method: string,
  schemaPath: string,
  status: number | null,
  body: string
): void {
  if (schemaPath === HEALTH_PATH) {
    startedAt.delete(id)
    return
  }
  useRequestLog.getState().record({
    method,
    path: schemaPath,
    status,
    durationMs: takeDuration(id),
    body: body.length > MAX_BODY ? `${body.slice(0, MAX_BODY)}\n… truncated` : body
  })
}

async function responseBody(response: Response): Promise<string> {
  const type = response.headers.get('content-type') ?? ''
  if (type.includes('text/event-stream')) {
    return ''
  }
  try {
    return await response.clone().text()
  } catch {
    return ''
  }
}

/** True when this process cancelled the fetch, including the 10-second timeout. */
function isClientAbort(error: unknown): boolean {
  if (!error || typeof error !== 'object') {
    return false
  }
  if ('name' in error) {
    const name = String(error.name)
    if (name === 'AbortError' || name === 'TimeoutError') {
      return true
    }
  }
  if ('message' in error) {
    const message = String(error.message)
    return (
      message === 'Fetch is aborted' ||
      message === 'The operation was aborted.' ||
      message === 'The user aborted a request.'
    )
  }
  return false
}

function failureMessage(method: string, schemaPath: string, detail: string): string {
  return `${method} ${schemaPath} failed (${detail})`
}

/**
 * Writes one audit-log row for an HTTP response outside 2xx, and for a fetch
 * that throws. A client abort, including the 10-second timeout, is not a
 * response and is left out. The shell notice is left to the caller that
 * already explains the failure.
 */
export const auditFailedRequests: Middleware = {
  onRequest({ id }) {
    noteStart(id)
  },
  async onResponse({ id, request, response, schemaPath }) {
    recordRequest(id, request.method, schemaPath, response.status, await responseBody(response))
    if (response.ok) {
      return
    }
    useErrorLog
      .getState()
      .record(failureMessage(request.method, schemaPath, String(response.status)))
  },
  onError({ id, request, schemaPath, error }) {
    const detail = error instanceof Error && error.message ? error.message : 'request failed'
    recordRequest(id, request.method, schemaPath, null, detail)
    if (isClientAbort(error)) {
      return
    }
    useErrorLog.getState().record(failureMessage(request.method, schemaPath, detail))
  }
}
