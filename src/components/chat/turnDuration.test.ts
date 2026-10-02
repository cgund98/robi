import { describe, expect, it } from 'vitest'

import { pendingSeconds, uuidV7Millis, workedLabel } from './turnDuration'

function idAt(ms: number): string {
  const hex = ms.toString(16).padStart(12, '0')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7000-8000-000000000000`
}

describe('workedLabel', () => {
  it('reads the span between the user message and the last reply', () => {
    expect(uuidV7Millis(idAt(1_700_000_000_000))).toBe(1_700_000_000_000)
    expect(workedLabel(idAt(1_000), idAt(1_000 + 12_400))).toBe('Worked for 12s')
    expect(workedLabel(idAt(1_000), idAt(1_000 + 125_000))).toBe('Worked for 2m 5s')
    expect(pendingSeconds(idAt(1_000), 1_000 + 4_900)).toBe(4)
    expect(workedLabel(idAt(5_000), idAt(1_000))).toBeNull()
  })
})
