export type ScaleAction = 'in' | 'out' | 'reset'

/** Which UI-scale gesture a key event is, or null. */
export function scaleShortcut(
  event: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey'>,
  isMac: boolean
): ScaleAction | null {
  const mod = isMac ? event.metaKey : event.ctrlKey
  if (!mod || event.altKey) {
    return null
  }
  switch (event.key) {
    case '+':
    case '=':
      return 'in'
    case '-':
    case '_':
      return 'out'
    case '0':
      return 'reset'
    default:
      return null
  }
}
