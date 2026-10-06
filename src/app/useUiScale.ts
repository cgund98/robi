import { useEffect } from 'react'

import { useUiScaleStore } from '../state/uiScaleStore'

export type ScaleAction = 'in' | 'out' | 'reset'

/** Which UI-scale gesture a key event is, or null. Pure so it can be tested. */
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

/**
 * Cmd/Ctrl `+`, `-`, and `0` scale the interface. The platform check decides
 * whether that is the meta key or the control key. Installed once, on the
 * window, so it works on every route.
 */
export function useUiScaleShortcuts() {
  useEffect(() => {
    const isMac = /mac/i.test(navigator.platform || navigator.userAgent)
    function onKeyDown(event: KeyboardEvent) {
      const action = scaleShortcut(event, isMac)
      if (action === null) {
        return
      }
      event.preventDefault()
      const store = useUiScaleStore.getState()
      if (action === 'in') {
        store.zoomIn()
      } else if (action === 'out') {
        store.zoomOut()
      } else {
        store.reset()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])
}
