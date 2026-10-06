import { describe, expect, it } from 'vitest'

import { sequenceSource, withIntrinsicSize } from './mermaid'

describe('mermaid diagrams', () => {
  it('copies a percentage-sized viewBox onto the root', () => {
    const svg = withIntrinsicSize(
      '<svg id="m" width="100%" style="max-width: 412px;" viewBox="0 -10 412 640"></svg>'
    )
    const root = new DOMParser().parseFromString(svg, 'image/svg+xml').documentElement
    expect(root.getAttribute('width')).toBe('412')
    expect(root.getAttribute('height')).toBe('640')
    expect(root.getAttribute('style')).toBe('max-width: 412px;')
  })

  it('keeps a semicolon that starts the next sequence statement', () => {
    const source = 'sequenceDiagram\n  A->>B: hello; B->>A: again\n'
    expect(sequenceSource(source)).toBe(source)
  })

  it('rewrites a semicolon that sits inside a note', () => {
    const source = 'sequenceDiagram\n  Note over A: fails as unknown;<br/>then continues\n'
    expect(sequenceSource(source)).toBe(
      'sequenceDiagram\n  Note over A: fails as unknown\uFF1B<br/>then continues\n'
    )
  })

  it('parses a sequence note that continues after a semicolon', async () => {
    const mermaid = (await import('mermaid')).default
    await mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' })
    const source = [
      'sequenceDiagram',
      '  A->>A: read <workspace>/.robi/mcp.json',
      '  Note over A: fails as unknown;<br/>then continues',
      '  alt session allow list has { server, tool }',
      '    A->>A: append { server, tool } to mcp_allows',
      '  end',
      '  C->>R: register mcp_<server>_<tool>'
    ].join('\n')
    await mermaid.parse(sequenceSource(source))
  })

  it('quotes an actor id that is the loop keyword', async () => {
    const mermaid = (await import('mermaid')).default
    await mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' })
    const source = [
      'sequenceDiagram',
      '    autonumber',
      '    actor User',
      '    participant UI as Solid UI',
      '    participant API as robi-api',
      '    participant Loop as robi-core agent loop',
      '    participant Model as Model provider',
      '    participant Store as MessageStore',
      '    User->>UI: Send a prompt',
      '    UI->>API: POST /api/v1/chat/messages',
      '    API->>Loop: run_turn(session, message)',
      '    Loop->>Store: append(user message)',
      '    Store-->>Loop: ok',
      '    Loop->>Model: stream(transcript)',
      '    Model-->>Loop: token deltas',
      '    Loop-->>API: Event::Delta',
      '    API-->>UI: SSE /api/v1/events/stream',
      '    UI-->>User: Render streaming text',
      '    Model-->>Loop: end of stream',
      '    Loop->>Store: append(assistant message)',
      '    Loop-->>API: Event::TurnComplete',
      '    API-->>UI: SSE turn-complete',
      '    UI-->>User: Finalize message'
    ].join('\n')
    const prepared = sequenceSource(source)
    expect(prepared).toContain('participant "Loop" as robi-core agent loop')
    expect(prepared).toContain('API->>"Loop": run_turn(session, message)')
    expect(prepared).not.toContain('"robi-core agent loop"')
    await mermaid.parse(prepared)
  })

  it('leaves a flowchart semicolon alone', () => {
    const source = 'flowchart LR\n  A-->B; C-->D\n'
    expect(sequenceSource(source)).toBe(source)
  })

  it('leaves a diagram that already has a pixel size alone', () => {
    const source = '<svg width="80" height="40" viewBox="0 0 80 40"></svg>'
    expect(withIntrinsicSize(source)).toBe(source)
  })
})
