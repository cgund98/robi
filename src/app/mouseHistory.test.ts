import { describe, expect, it } from 'vitest'

import { historyEnd, historyStep } from './mouseHistory'

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

describe('historyEnd', () => {
  it('grows on a push and keeps the forward stack on a pop', () => {
    expect(historyEnd('PUSH', 2, 1)).toBe(2)
    expect(historyEnd('POP', 1, 2)).toBe(2)
  })

  it('drops the forward stack when a new page is pushed', () => {
    expect(historyEnd('PUSH', 1, 4)).toBe(1)
  })

  it('leaves a replace in place', () => {
    expect(historyEnd('REPLACE', 1, 3)).toBe(3)
  })
})
