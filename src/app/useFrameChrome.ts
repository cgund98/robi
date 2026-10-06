import { isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { createRenderEffect, onCleanup, onMount } from 'solid-js'
import { useLocation, useNavigate } from '@solidjs/router'

import { historyStep } from './mouseHistory'
import { scaleShortcut } from './scaleShortcut'
import { uiScale } from '../state/uiScaleStore'

let historyEndIndex = 0
let historySeq = 0

const HISTORY_MARK = '_robi'

/**
 * Index of the current history entry.
 *
 * The hash router stores `_depth` as `history.length - 1`. Once the session
 * history stops growing, every new document gets that same depth, and Forward
 * stays disabled after Back. A push stamps `_robi` with its own number so
 * document steps stay distinct.
 */
export function solidHistoryIndex(): number {
  const marker = window.history.state?.[HISTORY_MARK]
  if (typeof marker === 'number') {
    return marker
  }
  const depth = window.history.state?._depth
  return typeof depth === 'number' ? depth : 0
}

/** Remember the furthest entry. Back does not shrink it; a new push replaces it. */
export function noteSolidHistory() {
  const index = solidHistoryIndex()
  if (index > historyEndIndex) {
    historyEndIndex = index
  }
}

function historyRecord(state: unknown): Record<string, unknown> {
  return state !== null && typeof state === 'object' ? { ...(state as object) } : {}
}

/** Stamp pushes so query-only document changes are real back/forward steps. */
function installHistoryMarks() {
  const current = history.pushState as History['pushState'] & { robi?: boolean }
  if (current.robi) {
    return
  }
  const here = solidHistoryIndex()
  if (historySeq < here) {
    historySeq = here
  }
  if (historyEndIndex < here) {
    historyEndIndex = here
  }
  const push = history.pushState.bind(history)
  const replace = history.replaceState.bind(history)
  const markedPush: History['pushState'] & { robi?: boolean } = (state, title, url) => {
    historySeq += 1
    if (historySeq > historyEndIndex) {
      historyEndIndex = historySeq
    }
    const next = historyRecord(state)
    next[HISTORY_MARK] = historySeq
    if (url === undefined) {
      push(next, title)
    } else {
      push(next, title, url)
    }
  }
  markedPush.robi = true
  history.pushState = markedPush
  history.replaceState = (state, title, url) => {
    const next = historyRecord(state)
    const marker = window.history.state?.[HISTORY_MARK]
    if (typeof marker === 'number' && typeof next[HISTORY_MARK] !== 'number') {
      next[HISTORY_MARK] = marker
    }
    if (url === undefined) {
      replace(next, title)
    } else {
      replace(next, title, url)
    }
  }
}

installHistoryMarks()

export function canSolidGoForward(index: number): boolean {
  return index < historyEndIndex
}

/** Side-button history and the UI-scale shortcuts, once per window frame. */
export function useFrameChrome() {
  const navigate = useNavigate()
  const location = useLocation()

  createRenderEffect(() => {
    // Query-only steps, such as opening another document, do not change the
    // path. `location.key` is always empty here, so the search is the signal.
    location.pathname
    location.search
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
