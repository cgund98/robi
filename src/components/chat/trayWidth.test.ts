import { afterEach, describe, expect, it } from 'vitest'

import {
  clampTrayWidth,
  readTrayWidth,
  TRAY_MAX_WIDTH,
  TRAY_MIN_WIDTH,
  trayMaxWidth,
  writeTrayWidth
} from './trayWidth'

describe('trayWidth', () => {
  afterEach(() => {
    localStorage.clear()
  })

  it('clamps within the min and the viewport-aware max', () => {
    // A wide viewport caps at the absolute max.
    expect(trayMaxWidth(4000)).toBe(TRAY_MAX_WIDTH)
    expect(clampTrayWidth(5000, 4000)).toBe(TRAY_MAX_WIDTH)
    expect(clampTrayWidth(1, 4000)).toBe(TRAY_MIN_WIDTH)

    // A narrow viewport leaves room to the left of the tray.
    expect(trayMaxWidth(800)).toBe(480)
    expect(clampTrayWidth(900, 800)).toBe(480)

    // Never below the minimum, even on a tiny viewport.
    expect(trayMaxWidth(400)).toBe(TRAY_MIN_WIDTH)
    expect(clampTrayWidth(500, 400)).toBe(TRAY_MIN_WIDTH)
  })

  it('falls back to the default for a non-finite value', () => {
    expect(clampTrayWidth(Number.NaN, 4000)).toBe(420)
  })

  it('reads the stored width and repairs a bad one', () => {
    expect(readTrayWidth(4000)).toBe(420)
    writeTrayWidth(640)
    expect(readTrayWidth(4000)).toBe(640)
    localStorage.setItem('robi.docsTrayWidth', 'nonsense')
    expect(readTrayWidth(4000)).toBe(420)
  })
})
