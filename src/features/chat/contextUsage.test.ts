import { describe, expect, it } from 'vitest'

import { estimateContext, type ContextMessage } from './contextUsage'

function assistant(content: string, input: number, output = 1, cached = 0): ContextMessage {
  return { content, usage: { input, output, cached }, tool_calls: [] }
}

describe('estimateContext', () => {
  it('stays empty until a model turn reports input', () => {
    const estimate = estimateContext(
      [{ content: 'hello', usage: { input: 0, output: 3, cached: 0 } }],
      'draft',
      8_000
    )
    expect(estimate.percent).toBeNull()
    expect(estimate.used).toBeNull()
    expect(estimate.window).toBe(8_000)
  })

  it('uses the latest input count and estimates text the provider has not counted', () => {
    const messages: ContextMessage[] = [
      assistant('old', 100),
      assistant('reply', 1_000, 20, 40),
      { content: 'tool output!!!!', tool_calls: [] }
    ]
    const estimate = estimateContext(messages, 'draft', 10_000, 'echo')
    const chars = 'reply'.length + 'tool output!!!!'.length + 'draft'.length + 'echo'.length
    expect(estimate.reported).toBe(1_000)
    expect(estimate.uncounted).toBe(Math.floor(chars / 4))
    expect(estimate.used).toBe(1_000 + Math.floor(chars / 4))
    expect(estimate.percent).toBe(Math.floor(((estimate.used ?? 0) * 100) / 10_000))
    expect(estimate.lastTurn).toEqual({ input: 1_000, output: 20, cached: 40 })
  })

  it('counts tool names and arguments, not a result stored on the call', () => {
    const messages: ContextMessage[] = [
      {
        content: 'x',
        usage: { input: 100, output: 1, cached: 0 },
        tool_calls: [{ name: 'read', args: { path: 'a.ts' } }]
      }
    ]
    const estimate = estimateContext(messages, '', 1_000)
    const chars = 'x'.length + 'read'.length + JSON.stringify({ path: 'a.ts' }).length
    expect(estimate.uncounted).toBe(Math.floor(chars / 4))
  })

  it('caps the ring at 100 percent and leaves it empty when the window is missing', () => {
    const messages = [assistant('hi', 500)]
    expect(estimateContext(messages, '', 100).percent).toBe(100)
    const missing = estimateContext(messages, '', null)
    expect(missing.percent).toBeNull()
    expect(missing.used).toBeGreaterThan(0)
    expect(missing.window).toBeNull()
  })
})
