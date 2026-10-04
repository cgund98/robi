import type { Middleware } from 'openapi-fetch'

import { useErrorLog } from '../state/errorLog'

function failureMessage(method: string, schemaPath: string, detail: string): string {
  return `${method} ${schemaPath} failed (${detail})`
}

/**
 * Writes one audit-log row for an HTTP response outside 2xx, and for a fetch
 * that throws. The shell notice is left to the caller that already explains
 * the failure.
 */
export const auditFailedRequests: Middleware = {
  onResponse({ request, response, schemaPath }) {
    if (response.ok) {
      return
    }
    useErrorLog
      .getState()
      .record(failureMessage(request.method, schemaPath, String(response.status)))
  },
  onError({ request, schemaPath, error }) {
    const detail = error instanceof Error && error.message ? error.message : 'request failed'
    useErrorLog.getState().record(failureMessage(request.method, schemaPath, detail))
  }
}
