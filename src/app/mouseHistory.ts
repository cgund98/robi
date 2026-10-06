/** Back and forward side buttons. Primary is 0, so these stay out of clicks. */
export function historyStep(button: number): -1 | 1 | null {
  if (button === 3) {
    return -1
  }
  if (button === 4) {
    return 1
  }
  return null
}

/** Index of the current entry in the router history stack. */
export function historyIndex(): number {
  const idx = window.history.state?.idx
  return typeof idx === 'number' ? idx : 0
}

/**
 * Furthest index still reachable forward. A push replaces anything ahead of
 * the current entry, so the end moves back with it.
 */
export function historyEnd(action: 'POP' | 'PUSH' | 'REPLACE', index: number, end: number): number {
  if (action === 'PUSH' || index > end) {
    return index
  }
  return end
}

/** Furthest index still reachable. The window frame keeps this across page changes. */
let historyEndIndex = 0

export function noteHistory(action: 'POP' | 'PUSH' | 'REPLACE', index: number) {
  historyEndIndex = historyEnd(action, index, historyEndIndex)
}

export function canGoForward(index: number): boolean {
  return index < historyEndIndex
}
