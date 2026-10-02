import { afterEach, describe, expect, it, vi } from 'vitest'

import { pickWorkspaceRoot } from './pickWorkspaceRoot'

describe('pickWorkspaceRoot', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    delete (globalThis as { isTauri?: boolean }).isTauri
  })

  it('returns the trimmed path from the prompt outside the desktop shell', async () => {
    window.prompt = vi.fn(() => '  /tmp/robi  ')
    await expect(pickWorkspaceRoot()).resolves.toBe('/tmp/robi')
  })

  it('returns null when the prompt is cancelled or blank', async () => {
    window.prompt = vi.fn(() => null)
    await expect(pickWorkspaceRoot()).resolves.toBeNull()
    window.prompt = vi.fn(() => '   ')
    await expect(pickWorkspaceRoot()).resolves.toBeNull()
  })
})
