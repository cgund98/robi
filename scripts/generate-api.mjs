#!/usr/bin/env node
/**
 * Regenerate OpenAPI types for openapi-fetch from the committed Rust spec.
 *
 * Prefer refreshing the JSON first when routes change:
 *   make openapi-spec && pnpm run generate:api
 *
 * Usage (repo root): pnpm run generate:api
 */

import { spawnSync } from 'node:child_process'
import process from 'node:process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const openapiPath = path.join(root, 'openapi/openapi.json')
const schemaPath = path.join(root, 'src/api/schema.d.ts')

const result = spawnSync('pnpm', ['exec', 'openapi-typescript', openapiPath, '-o', schemaPath], {
  cwd: root,
  stdio: 'inherit'
})

if (result.error) {
  throw result.error
}
if (result.status !== 0) {
  process.exit(result.status ?? 1)
}
