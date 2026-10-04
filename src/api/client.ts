import { invoke, isTauri } from '@tauri-apps/api/core'
import createClient from 'openapi-fetch'

import { auditFailedRequests } from './auditRequests'
import type { paths } from './schema'

/**
 * Origin of the API, without a trailing slash.
 *
 * Empty uses the Vite `/api` proxy (web-only dev, or `ROBI_EXTERNAL_API=1`).
 * Inside the desktop app this becomes `http://127.0.0.1:<port>` after
 * `resolveApiBase`.
 */
let apiBase = import.meta.env.VITE_API_BASE_URL ?? ''

export function apiBaseUrl(): string {
  return apiBase
}

/** Ask the Tauri process which port it bound. No-op in a normal browser. */
export async function resolveApiBase(): Promise<void> {
  if (!isTauri()) {
    return
  }
  const origin = await invoke<string>('api_base_url')
  apiBase = origin.replace(/\/$/, '')
}

/** Absolute form of a path-only URL. WebKit rejects `new Request('/api/...')` when the page is not an http origin. */
function absoluteApiUrl(input: RequestInfo | URL): RequestInfo | URL {
  if (!apiBase || typeof input !== 'string' || !input.startsWith('/')) {
    return input
  }
  return `${apiBase}${input}`
}

/**
 * `Request` used by the API client.
 *
 * The desktop page is not served over http. A relative `/api/...` URL then
 * throws "The string did not match the expected pattern." Resolve it against
 * the bound API origin first.
 */
class ApiRequest extends Request {
  constructor(input: RequestInfo | URL, init?: RequestInit) {
    super(absoluteApiUrl(input), init)
  }
}

/** Bound on every API fetch. The event stream is an EventSource and is not covered. */
const API_TIMEOUT_MS = 10_000

/**
 * `fetch` with a 10-second abort. A caller signal still cancels the request;
 * whichever fires first wins.
 */
export function fetchWithTimeout(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const timeout = AbortSignal.timeout(API_TIMEOUT_MS)
  const signal = init?.signal ? AbortSignal.any([init.signal, timeout]) : timeout
  return fetch(input, { ...init, signal })
}

async function fetchWithBase(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  if (!apiBase) {
    return fetchWithTimeout(input, init)
  }
  const request = input instanceof Request ? input : new ApiRequest(input, init)
  const url = new URL(request.url)
  const base = new URL(apiBase)
  url.protocol = base.protocol
  url.host = base.host
  const method = request.method
  // A copied Request exposes its body as a ReadableStream, which WebKit will not
  // upload. Fetching that Request object itself fails with "Load failed".
  // Send the URL and the body text instead.
  const body = method === 'GET' || method === 'HEAD' ? undefined : await request.text()
  return fetchWithTimeout(url, {
    method,
    headers: request.headers,
    body,
    credentials: request.credentials,
    cache: request.cache,
    redirect: request.redirect,
    referrer: request.referrer,
    integrity: request.integrity,
    signal: request.signal
  })
}

/**
 * Typed HTTP client for `robi-api`.
 *
 * Types come from `schema.d.ts` (openapi-typescript). Regenerate after API changes:
 * `make openapi-spec && pnpm run generate:api`
 */
export const api = createClient<paths>({
  baseUrl: import.meta.env.VITE_API_BASE_URL ?? '',
  fetch: fetchWithBase,
  Request: ApiRequest
})

api.use(auditFailedRequests)

export type { paths }
