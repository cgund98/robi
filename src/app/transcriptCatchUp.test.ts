import { describe, expect, it } from 'vitest'

import { TRANSCRIPT_CATCH_UP_MS, transcriptCatchUpDue } from './transcriptCatchUp'

describe('transcriptCatchUpDue', () => {
  it('refetches a busy phase that has gone quiet', () => {
    expect(transcriptCatchUpDue('thinking', TRANSCRIPT_CATCH_UP_MS)).toBe(true)
    expect(transcriptCatchUpDue('responding', TRANSCRIPT_CATCH_UP_MS + 1)).toBe(true)
  })

  it('leaves a live stream and an idle session alone', () => {
    expect(transcriptCatchUpDue('thinking', TRANSCRIPT_CATCH_UP_MS - 1)).toBe(false)
    expect(transcriptCatchUpDue('idle', TRANSCRIPT_CATCH_UP_MS)).toBe(false)
    expect(transcriptCatchUpDue(undefined, TRANSCRIPT_CATCH_UP_MS)).toBe(false)
  })
})
