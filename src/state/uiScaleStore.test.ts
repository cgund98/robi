import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  applyUiScale,
  clampUiScale,
  formatUiScale,
  readStoredUiScale,
  stepUiScale,
  UI_SCALE_DEFAULT,
  useUiScaleStore,
  writeStoredUiScale
} from './uiScaleStore'

const { setZoom } = vi.hoisted(() => ({ setZoom: vi.fn() }))

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ setZoom })
}))

function setTauri(on: boolean) {
  if (on) {
    ;(globalThis as { isTauri?: boolean }).isTauri = true
  } else {
    delete (globalThis as { isTauri?: boolean }).isTauri
  }
}

describe('uiScaleStore', () => {
  afterEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('zoom')
    setTauri(false)
    vi.clearAllMocks()
  })

  it('snaps a value onto the ladder and defaults a non-finite one', () => {
    expect(clampUiScale(1.06)).toBe(1.1)
    expect(clampUiScale(1.3)).toBe(1.25)
    expect(clampUiScale(Number.NaN)).toBe(UI_SCALE_DEFAULT)
    expect(clampUiScale(0.6)).toBe(0.67)
  })

  it('steps up and down and holds at either end', () => {
    expect(stepUiScale(1, 1)).toBe(1.1)
    expect(stepUiScale(1, -1)).toBe(0.9)
    expect(stepUiScale(3, 1)).toBe(3)
    expect(stepUiScale(0.5, -1)).toBe(0.5)
  })

  it('formats the level as a percentage', () => {
    expect(formatUiScale(1)).toBe('100%')
    expect(formatUiScale(1.1)).toBe('110%')
  })

  it('reads the stored scale and repairs a missing or bad one', () => {
    expect(readStoredUiScale()).toBe(UI_SCALE_DEFAULT)
    writeStoredUiScale(1.5)
    expect(readStoredUiScale()).toBe(1.5)
    localStorage.setItem('robi.uiScale', 'nonsense')
    expect(readStoredUiScale()).toBe(UI_SCALE_DEFAULT)
    localStorage.setItem('robi.uiScale', '1.3')
    expect(readStoredUiScale()).toBe(1.25)
  })

  it('applies the scale as CSS zoom in a plain browser', async () => {
    await applyUiScale(1.5)
    expect(document.documentElement.style.getPropertyValue('zoom')).toBe('1.5')
    await applyUiScale(1)
    expect(document.documentElement.style.getPropertyValue('zoom')).toBe('')
  })

  it('applies the scale through the webview in the desktop app', async () => {
    setTauri(true)
    await applyUiScale(1.25)
    expect(setZoom).toHaveBeenCalledWith(1.25)
  })

  it('writes storage and applies the scale through the store', () => {
    useUiScaleStore.getState().setScale(1.5)
    expect(useUiScaleStore.getState().scale).toBe(1.5)
    expect(localStorage.getItem('robi.uiScale')).toBe('1.5')
    expect(document.documentElement.style.getPropertyValue('zoom')).toBe('1.5')

    useUiScaleStore.getState().zoomOut()
    expect(useUiScaleStore.getState().scale).toBe(1.25)

    useUiScaleStore.getState().reset()
    expect(useUiScaleStore.getState().scale).toBe(UI_SCALE_DEFAULT)
    expect(document.documentElement.style.getPropertyValue('zoom')).toBe('')
  })
})
