import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect } from 'react'
import { useLocation, useNavigate, useNavigationType } from 'react-router-dom'

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

/**
 * The webview does not map the side buttons onto its history, so the shell does.
 * On macOS those buttons never reach the page: the desktop process emits
 * `mouse-history` instead. A browser still delivers them as mouse buttons 3 and 4.
 * Each document the docs viewer opens is its own history entry, and so is every
 * route, which means one gesture walks both.
 */
export function useMouseHistory() {
  const navigate = useNavigate()
  const action = useNavigationType()
  useLocation()
  noteHistory(action, historyIndex())

  useEffect(() => {
    const onMouseDown = (event: MouseEvent) => {
      const step = historyStep(event.button)
      if (step === null) {
        return
      }
      event.preventDefault()
      navigate(step)
    }
    window.addEventListener('mousedown', onMouseDown)

    let unlistened = false
    let unlisten: (() => void) | undefined
    if (isTauri()) {
      void listen<number>('mouse-history', (event) => {
        if (event.payload === -1 || event.payload === 1) {
          navigate(event.payload)
        }
      }).then((stop) => {
        if (unlistened) {
          stop()
        } else {
          unlisten = stop
        }
      })
    }

    return () => {
      unlistened = true
      unlisten?.()
      window.removeEventListener('mousedown', onMouseDown)
    }
  }, [navigate])
}
