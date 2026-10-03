import { describe, expect, it } from 'vitest'

import { fetchStillCurrent, startFetch } from './latestFetch'

describe('latestFetch', () => {
  it('keeps only the fetch that started later for the same key', () => {
    const first = startFetch('message:s:m')
    const second = startFetch('message:s:m')
    expect(fetchStillCurrent('message:s:m', first)).toBe(false)
    expect(fetchStillCurrent('message:s:m', second)).toBe(true)
  })

  it('does not retire a fetch to a different key', () => {
    const message = startFetch('message:s:m')
    startFetch('session:s')
    expect(fetchStillCurrent('message:s:m', message)).toBe(true)
  })
})
