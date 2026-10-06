import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { createRenderEffect, onCleanup, onMount } from 'solid-js'
import { useLocation, useNavigate } from '@solidjs/router'

import { historyStep } from '../../app/mouseHistory'
import { scaleShortcut } from '../../app/useUiScale'
import { uiScale } from '../state/host'

let historyEndIndex = 0

/** Index Solid's hash router stores on `history.state._depth`. */
export function solidHistoryIndex(): number {
  const depth = window.history.state?._depth
  return typeof depth === 'number' ? depth : 0
}

/**
 * A push lands on the tip of `history.length` and drops anything that was
 * ahead. Back and forward move the depth without changing that tip.
 */
export function noteSolidHistory() {
  const index = solidHistoryIndex()
  const atTip = window.history.length - 1 === index
  if (atTip || index > historyEndIndex) {
    historyEndIndex = index
  }
}

export function canSolidGoForward(index: number): boolean {
  return index < historyEndIndex
}

/** Side-button history and the UI-scale shortcuts, once per window frame. */
export function useFrameChrome() {
  const navigate = useNavigate()
  const location = useLocation()

  createRenderEffect(() => {
    location.pathname
    location.key
    noteSolidHistory()
  })

  onMount(() => {
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

    onCleanup(() => {
      unlistened = true
      unlisten?.()
      window.removeEventListener('mousedown', onMouseDown)
    })
  })

  onMount(() => {
    const isMac = /mac/i.test(navigator.platform || navigator.userAgent)
    function onKeyDown(event: KeyboardEvent) {
      const action = scaleShortcut(event, isMac)
      if (action === null) {
        return
      }
      event.preventDefault()
      if (action === 'in') {
        uiScale.zoomIn()
      } else if (action === 'out') {
        uiScale.zoomOut()
      } else {
        uiScale.reset()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })
}
