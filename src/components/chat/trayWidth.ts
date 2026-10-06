/** Width rules for the docs chat tray's drag handle. */

export const TRAY_MIN_WIDTH = 300
export const TRAY_DEFAULT_WIDTH = 420
export const TRAY_MAX_WIDTH = 900
/** Space kept to the left of the tray so the page is never fully covered. */
const TRAY_MIN_VISIBLE = 320

/** The widest the tray may be on this viewport without hiding the page. */
export function trayMaxWidth(viewportWidth: number): number {
  return Math.max(TRAY_MIN_WIDTH, Math.min(TRAY_MAX_WIDTH, viewportWidth - TRAY_MIN_VISIBLE))
}

export function clampTrayWidth(value: number, viewportWidth: number): number {
  const max = trayMaxWidth(viewportWidth)
  if (!Number.isFinite(value)) {
    return Math.min(TRAY_DEFAULT_WIDTH, max)
  }
  return Math.min(Math.max(value, TRAY_MIN_WIDTH), max)
}

const STORAGE_KEY = 'robi.docsTrayWidth'

export function readTrayWidth(viewportWidth: number): number {
  let stored: number | null
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    stored = raw === null ? null : Number(raw)
  } catch {
    stored = null
  }
  if (stored === null || !Number.isFinite(stored) || stored <= 0) {
    return clampTrayWidth(TRAY_DEFAULT_WIDTH, viewportWidth)
  }
  return clampTrayWidth(stored, viewportWidth)
}

export function writeTrayWidth(width: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(Math.round(width)))
  } catch {
    // A blocked storage backend only costs the remembered width.
  }
}
