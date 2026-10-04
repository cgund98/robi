import { describe, expect, it } from 'vitest'

import { historyStep } from './mouseHistory'

describe('historyStep', () => {
  it('maps the side buttons onto history', () => {
    expect(historyStep(3)).toBe(-1)
    expect(historyStep(4)).toBe(1)
  })

  it('leaves ordinary clicks alone', () => {
    expect(historyStep(0)).toBeNull()
    expect(historyStep(1)).toBeNull()
    expect(historyStep(2)).toBeNull()
  })
})
