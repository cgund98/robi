import { render } from '@testing-library/react'
import { useState } from 'react'
import { describe, expect, it } from 'vitest'

import { useReconnectingEventSource } from './useReconnectingEventSource'

class FakeEventSource {
  static instances: FakeEventSource[] = []

  url: string
  closed = false
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  private listeners = new Map<string, Array<(event: Event) => void>>()

  constructor(url: string) {
    this.url = url
    FakeEventSource.instances.push(this)
  }

  addEventListener(type: string, listener: (event: Event) => void): void {
    const list = this.listeners.get(type) ?? []
    list.push(listener)
    this.listeners.set(type, list)
  }

  close(): void {
    this.closed = true
  }

  open(): void {
    this.onopen?.(new Event('open'))
  }

  fail(): void {
    this.onerror?.(new Event('error'))
  }

  emit(type: string, data: string): void {
    const event = { data } as MessageEvent
    if (type === 'message') {
      this.onmessage?.(event)
    }
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event)
    }
  }
}

function createFake(url: string): EventSource {
  return new FakeEventSource(url) as unknown as EventSource
}

function Probe({ url, onEvent }: { url: string | null; onEvent: (data: string) => void }) {
  useReconnectingEventSource({
    url,
    eventTypes: ['robi.agent.v1.turn_started', 'robi.agent.v1.message_added'],
    reconnectDelayMs: 0,
    createEventSource: createFake,
    onEvent: (event) => onEvent(String(event.data))
  })
  return null
}

describe('useReconnectingEventSource', () => {
  it('applies one envelope and ignores a frame from a previous epoch', () => {
    FakeEventSource.instances = []
    const seen: string[] = []
    const firstUrl = '/api/v1/events/stream?session_id=a'
    const secondUrl = '/api/v1/events/stream?session_id=b'

    const { rerender } = render(<Probe url={firstUrl} onEvent={(data) => seen.push(data)} />)
    const first = FakeEventSource.instances[0]
    expect(first.url).toBe(firstUrl)
    first.open()
    first.emit('robi.agent.v1.turn_started', '{"type":"robi.agent.v1.turn_started"}')

    rerender(<Probe url={secondUrl} onEvent={(data) => seen.push(data)} />)
    const second = FakeEventSource.instances[1]
    expect(first.closed).toBe(true)

    first.emit('robi.agent.v1.turn_started', '{"type":"stale"}')
    second.open()
    second.emit('robi.agent.v1.message_added', '{"type":"robi.agent.v1.message_added"}')

    expect(seen).toEqual([
      '{"type":"robi.agent.v1.turn_started"}',
      '{"type":"robi.agent.v1.message_added"}'
    ])
  })

  it('reconnects after an error and marks the next open as a reconnect', async () => {
    FakeEventSource.instances = []
    const opens: boolean[] = []

    function Harness() {
      const [url] = useState('/api/v1/events/stream?session_id=a')
      useReconnectingEventSource({
        url,
        reconnectDelayMs: 0,
        createEventSource: createFake,
        onOpen: ({ reconnected }) => opens.push(reconnected)
      })
      return null
    }

    render(<Harness />)
    const first = FakeEventSource.instances[0]
    first.open()
    first.fail()
    await new Promise((resolve) => setTimeout(resolve, 20))
    const second = FakeEventSource.instances[1]
    expect(first.closed).toBe(true)
    second.open()
    expect(opens).toEqual([false, true])
  })
})
