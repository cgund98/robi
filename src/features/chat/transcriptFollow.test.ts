import { describe, expect, it } from 'vitest'

import { pinnedAfterScroll } from './transcriptFollow'

const base = {
  pinned: true,
  scrollTop: 400,
  previousScrollTop: 400,
  scrollHeight: 800,
  previousScrollHeight: 800,
  clientHeight: 400,
  threshold: 48
}

describe('pinnedAfterScroll', () => {
  it('stays pinned when a tool request grows the list past the viewport', () => {
    expect(
      pinnedAfterScroll({
        ...base,
        scrollHeight: 1600,
        previousScrollHeight: 800
      })
    ).toBe(true)
  })

  it('releases when the reader scrolls up', () => {
    expect(
      pinnedAfterScroll({
        ...base,
        scrollTop: 200,
        previousScrollTop: 400
      })
    ).toBe(false)
  })

  it('pins again at the bottom', () => {
    expect(
      pinnedAfterScroll({
        ...base,
        pinned: false,
        scrollTop: 400
      })
    ).toBe(true)
  })

  it('does not pull a reader back down when the list grows', () => {
    expect(
      pinnedAfterScroll({
        ...base,
        pinned: false,
        scrollTop: 100,
        previousScrollTop: 100,
        scrollHeight: 1600,
        previousScrollHeight: 800
      })
    ).toBe(false)
  })
})
