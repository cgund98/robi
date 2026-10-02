/** Milliseconds from a UUIDv7, or null when the id is not one. */
export function uuidV7Millis(id: string): number | null {
  const hex = id.replace(/-/g, '')
  if (hex.length < 12 || !/^[0-9a-fA-F]+$/.test(hex)) {
    return null
  }
  const millis = Number.parseInt(hex.slice(0, 12), 16)
  return Number.isFinite(millis) ? millis : null
}

/** `12s`, or `2m 5s` once a minute has passed. */
export function formatElapsed(seconds: number): string {
  if (seconds < 60) {
    return `${seconds}s`
  }
  const minutes = Math.floor(seconds / 60)
  const rest = seconds % 60
  if (rest === 0) {
    return `${minutes}m`
  }
  return `${minutes}m ${rest}s`
}

/** `Worked for 12s`, or `Worked for 2m 5s` once a minute has passed. */
export function workedLabel(startId: string, endId: string): string | null {
  const start = uuidV7Millis(startId)
  const end = uuidV7Millis(endId)
  if (start == null || end == null || end < start) {
    return null
  }
  const seconds = Math.max(1, Math.round((end - start) / 1000))
  return `Worked for ${formatElapsed(seconds)}`
}

/** Whole seconds the current turn has been pending, from its user message. */
export function pendingSeconds(startId: string | undefined, now: number): number | null {
  if (!startId) {
    return null
  }
  const start = uuidV7Millis(startId)
  if (start == null || now < start) {
    return null
  }
  return Math.floor((now - start) / 1000)
}
