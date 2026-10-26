import { describe, expect, it } from 'vitest'

import { filterSessions, RECENTS_CAP, sessionMark, visibleRecents } from './recents'

function session(
  overrides: Partial<{
    id: string
    title: string | null
    has_pending_agent: boolean
    turn_display: string
  }> = {}
) {
  return {
    id: overrides.id ?? 's1',
    title: overrides.title === undefined ? 'Fix the parser' : overrides.title,
    has_pending_agent: overrides.has_pending_agent ?? false,
    turn_display: overrides.turn_display ?? 'idle'
  }
}

describe('sessionMark', () => {
  it('marks a session awaiting approval ahead of running', () => {
    expect(
      sessionMark(
        session({ turn_display: 'awaiting_approval', has_pending_agent: true }),
        'thinking'
      )
    ).toBe('awaiting')
  })

  it('marks a pending or in-flight turn running', () => {
    expect(sessionMark(session({ turn_display: 'pending' }), undefined)).toBe('running')
    expect(sessionMark(session({ has_pending_agent: true }), undefined)).toBe('running')
    expect(sessionMark(session(), 'thinking')).toBe('running')
    expect(sessionMark(session(), 'responding')).toBe('running')
  })

  it('marks everything else idle', () => {
    expect(sessionMark(session(), undefined)).toBe('idle')
    expect(sessionMark(session(), 'idle')).toBe('idle')
    expect(sessionMark(session({ turn_display: 'failed' }), undefined)).toBe('idle')
  })
})

describe('filterSessions', () => {
  const sessions = [
    session({ id: 'a', title: 'Fix the parser' }),
    session({ id: 'b', title: 'Render the top bar' }),
    session({ id: 'c', title: null })
  ]

  it('matches the display title, case-insensitively', () => {
    expect(filterSessions(sessions, 'PARSE').map((item) => item.id)).toEqual(['a'])
    expect(filterSessions(sessions, ' bar ').map((item) => item.id)).toEqual(['b'])
  })

  it('matches the fallback title of an unnamed session', () => {
    expect(filterSessions(sessions, 'new chat').map((item) => item.id)).toEqual(['c'])
  })

  it('returns every row for an empty query', () => {
    expect(filterSessions(sessions, '  ')).toHaveLength(3)
  })

  it('returns nothing when no row matches', () => {
    expect(filterSessions(sessions, 'zzz')).toEqual([])
  })
})

describe('visibleRecents', () => {
  it('caps the list and reports what the cap hid', () => {
    const many = Array.from({ length: RECENTS_CAP + 3 }, (_, index) =>
      session({ id: `s${index}`, title: `Session ${index}` })
    )
    const { rows, hidden } = visibleRecents(many, '')
    expect(rows).toHaveLength(RECENTS_CAP)
    expect(hidden).toBe(3)
  })

  it('filters before it caps, so a narrow query shows every match', () => {
    const many = Array.from({ length: RECENTS_CAP + 3 }, (_, index) =>
      session({ id: `s${index}`, title: `Session ${index}` })
    )
    const { rows, hidden } = visibleRecents(many, 'Session 2')
    expect(rows.map((row) => row.id)).toEqual(['s2', 's20', 's21', 's22'])
    expect(hidden).toBe(0)
  })
})
