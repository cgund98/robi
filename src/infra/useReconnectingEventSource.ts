import { useEffect, useRef } from 'react'

const INITIAL_BACKOFF_MS = 500
const MAX_BACKOFF_MS = 15_000

type ReconnectingEventSourceOptions = {
  url: string | null
  eventTypes?: readonly string[]
  /** Fixed delay. When omitted, retries back off from 500ms to 15s. */
  reconnectDelayMs?: number
  createEventSource?: (url: string) => EventSource
  onEvent?: (event: MessageEvent) => void
  onOpen?: (info: { reconnected: boolean }) => void
}

/**
 * One EventSource for `url`. A null url stays disconnected.
 * Errors close the socket and retry. A url change advances the epoch so a
 * handler from the previous socket cannot apply.
 */
export function useReconnectingEventSource({
  url,
  eventTypes,
  reconnectDelayMs,
  createEventSource,
  onEvent,
  onOpen
}: ReconnectingEventSourceOptions): void {
  const epochRef = useRef(0)
  const onEventRef = useRef(onEvent)
  const onOpenRef = useRef(onOpen)
  const factoryRef = useRef(createEventSource)

  useEffect(() => {
    onEventRef.current = onEvent
    onOpenRef.current = onOpen
    factoryRef.current = createEventSource
  })

  const typesKey = eventTypes?.join('\n') ?? ''

  useEffect(() => {
    if (!url) {
      return
    }

    const epoch = epochRef.current + 1
    epochRef.current = epoch
    let source: EventSource | null = null
    let closed = false
    let opened = false
    let attempt = 0
    let timer: ReturnType<typeof setTimeout> | undefined

    const handleFrame = (event: Event) => {
      if (closed || epoch !== epochRef.current) {
        return
      }
      const frame = event as MessageEvent
      if (frame.data === undefined) {
        return
      }
      onEventRef.current?.(frame)
    }

    const connect = () => {
      if (closed || epoch !== epochRef.current) {
        return
      }
      const factory = factoryRef.current ?? ((nextUrl: string) => new EventSource(nextUrl))
      const next = factory(url)
      source = next
      next.onopen = () => {
        if (closed || epoch !== epochRef.current) {
          return
        }
        const reconnected = opened
        opened = true
        attempt = 0
        onOpenRef.current?.({ reconnected })
      }
      next.onerror = () => {
        next.close()
        if (source === next) {
          source = null
        }
        if (closed || epoch !== epochRef.current) {
          return
        }
        const delay =
          reconnectDelayMs ?? Math.min(INITIAL_BACKOFF_MS * 2 ** attempt, MAX_BACKOFF_MS)
        attempt += 1
        timer = setTimeout(connect, delay)
      }
      if (typesKey) {
        for (const eventType of typesKey.split('\n')) {
          next.addEventListener(eventType, handleFrame)
        }
      }
    }

    connect()

    return () => {
      closed = true
      clearTimeout(timer)
      source?.close()
    }
  }, [url, typesKey, reconnectDelayMs])
}
