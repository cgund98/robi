import { describe, expect, it } from 'vitest'

import { AGENT_EVENT_TYPES, buildAgentEventsStreamUrl, parseEventEnvelope } from './agentEvents'

const envelope = {
  specversion: '1.0',
  id: '018f2b2a-7c1e-7b2a-8c1e-7b2a8c1e7b2a',
  source: 'robi/agent',
  type: 'robi.agent.v1.turn_started',
  time: '2026-01-01T00:00:00.000Z',
  subject: '018f2b2a-7c1e-7b2a-8c1e-7b2a8c1e7b2b',
  data: { session_id: '018f2b2a-7c1e-7b2a-8c1e-7b2a8c1e7b2b' }
}

describe('agent events', () => {
  it('builds the stream url for the active session', () => {
    const url = buildAgentEventsStreamUrl(envelope.subject, '')
    const parsed = new URL(url, 'http://127.0.0.1')
    expect(parsed.pathname).toBe('/api/v1/events/stream')
    expect(parsed.searchParams.get('session_id')).toBe(envelope.subject)
    expect(parsed.searchParams.getAll('event_types')).toEqual([...AGENT_EVENT_TYPES])
  })

  it('parses one envelope and ignores a malformed frame', () => {
    expect(parseEventEnvelope(JSON.stringify(envelope))).toEqual(envelope)
    expect(parseEventEnvelope('not-json')).toBeNull()
    expect(parseEventEnvelope('{"type":"robi.agent.v1.turn_started"}')).toBeNull()
  })
})
