import createClient from 'openapi-fetch'

import type { paths } from './schema'

/**
 * Typed HTTP client for `robi-api`.
 *
 * Types come from `schema.d.ts` (openapi-typescript). Regenerate after API changes:
 * `make openapi-spec && pnpm run generate:api`
 *
 * Empty `baseUrl` uses the Vite `/api` proxy in web-only dev.
 */
export const api = createClient<paths>({
  baseUrl: import.meta.env.VITE_API_BASE_URL ?? ''
})

export type { paths }
