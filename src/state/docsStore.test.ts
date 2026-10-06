import { describe, expect, it } from 'vitest'

import { docs, patchDocs } from './docsStore'

describe('docsStore', () => {
  it('records a file change with a fresh seq so a memo re-runs', () => {
    patchDocs({ change: null })
    docs.record({ path: 'docs/a.md', source: 'agent', outcome: 'applied', sessionId: null })
    const first = docs.change?.seq ?? 0
    expect(docs.change?.path).toBe('docs/a.md')
    expect(docs.change?.source).toBe('agent')

    docs.record({ path: 'docs/a.md', source: 'agent', outcome: 'applied', sessionId: null })
    expect(docs.change?.seq).toBe(first + 1)
  })
})
