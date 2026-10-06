import { getCurrentWebview } from '@tauri-apps/api/webview'
import { isTauri } from '@tauri-apps/api/core'
import { create } from 'zustand'

/** Chrome's zoom ladder, so the steps feel the same as a browser's Cmd/Ctrl + and -. */
export const UI_SCALE_STEPS = [
  0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3
] as const

export const UI_SCALE_DEFAULT = 1

const STORAGE_KEY = 'robi.uiScale'

/** Snap any value onto the ladder. A non-finite value lands on 100%. */
export function clampUiScale(scale: number): number {
  if (!Number.isFinite(scale)) {
    return UI_SCALE_DEFAULT
  }
  return UI_SCALE_STEPS.reduce((nearest, step) =>
    Math.abs(step - scale) < Math.abs(nearest - scale) ? step : nearest
  )
}

/** The next or previous ladder step. Stays put at either end. */
export function stepUiScale(current: number, direction: -1 | 1): number {
  const index = UI_SCALE_STEPS.indexOf(clampUiScale(current) as (typeof UI_SCALE_STEPS)[number])
  const next = Math.min(Math.max(index + direction, 0), UI_SCALE_STEPS.length - 1)
  return UI_SCALE_STEPS[next]
}

/** The level as it is shown to a person, such as `"110%"`. */
export function formatUiScale(scale: number): string {
  return `${Math.round(clampUiScale(scale) * 100)}%`
}

export function readStoredUiScale(): number {
  let stored: number | null
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    stored = raw === null ? null : Number(raw)
  } catch {
    stored = null
  }
  if (stored === null || !Number.isFinite(stored) || stored <= 0) {
    return UI_SCALE_DEFAULT
  }
  return clampUiScale(stored)
}

export function writeStoredUiScale(scale: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(scale))
  } catch {
    // A blocked storage backend only costs the remembered scale.
  }
}

/**
 * Scale the whole interface.
 *
 * Inside the desktop app this is the webview's own zoom (`WKWebView.pageZoom`
 * on macOS), the same thing a browser's Cmd/Ctrl + does. In a plain browser the
 * page cannot zoom itself from script, so the CSS `zoom` property stands in.
 * Callers do not need to await; a failed zoom only costs the current render.
 */
export async function applyUiScale(scale: number): Promise<void> {
  const value = clampUiScale(scale)
  if (isTauri()) {
    await getCurrentWebview().setZoom(value)
    return
  }
  const root = document.documentElement
  if (value === UI_SCALE_DEFAULT) {
    root.style.removeProperty('zoom')
  } else {
    root.style.setProperty('zoom', String(value))
  }
}

type UiScaleState = {
  scale: number
  setScale: (scale: number) => void
  zoomIn: () => void
  zoomOut: () => void
  reset: () => void
}

function apply(scale: number) {
  writeStoredUiScale(scale)
  void applyUiScale(scale).catch(() => {
    // Keep the stored choice; the next apply may succeed.
  })
}

export const useUiScaleStore = create<UiScaleState>((set, get) => ({
  scale: readStoredUiScale(),

  setScale: (scale) => {
    const next = clampUiScale(scale)
    set({ scale: next })
    apply(next)
  },

  zoomIn: () => {
    const next = stepUiScale(get().scale, 1)
    set({ scale: next })
    apply(next)
  },

  zoomOut: () => {
    const next = stepUiScale(get().scale, -1)
    set({ scale: next })
    apply(next)
  },

  reset: () => {
    set({ scale: UI_SCALE_DEFAULT })
    apply(UI_SCALE_DEFAULT)
  }
}))
