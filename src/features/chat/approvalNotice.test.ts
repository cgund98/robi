import { describe, expect, it } from 'vitest'

import type { ChatToolCall } from './toolCallView'
import { approvalNoticeBody, claimPause, releasePause, windowInFront } from './approvalNotice'

function call(overrides: Partial<ChatToolCall> = {}): ChatToolCall {
  return {
    id: '1',
    name: 'edit_file',
    args: { path: 'src/foo.ts' },
    approval_status: 'pending',
    execution_status: 'not_started',
    ...overrides
  }
}

describe('approval notice', () => {
  it('claims a pause once', () => {
    const seen = new Set<string>()
    expect(claimPause(seen, 'session')).toBe(true)
    expect(claimPause(seen, 'session')).toBe(false)
    releasePause(seen, 'session')
    expect(claimPause(seen, 'session')).toBe(true)
  })

  it('skips the banner when the window is in front', () => {
    expect(windowInFront({ focused: true, minimized: false, visible: true })).toBe(true)
    expect(windowInFront({ focused: false, minimized: false, visible: true })).toBe(false)
    expect(windowInFront({ focused: true, minimized: true, visible: true })).toBe(false)
    expect(windowInFront({ focused: true, minimized: false, visible: false })).toBe(false)
  })

  it('uses the approval bar verb and target', () => {
    expect(approvalNoticeBody(call())).toBe('Edit src/foo.ts')
    expect(approvalNoticeBody(call({ name: 'shell', args: { command: 'cargo test' } }))).toBe(
      'Run cargo'
    )
    expect(approvalNoticeBody(undefined)).toBe('A tool is waiting')
  })
})
