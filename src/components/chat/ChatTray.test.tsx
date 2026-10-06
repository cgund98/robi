import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ChatTray } from './ChatTray'
import type { ChatPanelProps } from './ChatPanel'

// The tray chrome is under test; the chat surface itself has its own tests.
vi.mock('./ChatPanel', () => ({
  ChatPanel: () => <div data-testid="chat-panel" />
}))

const chatProps: ChatPanelProps = {
  messages: [],
  sessionId: 's1',
  echo: null,
  echoFiles: [],
  phase: 'idle',
  mode: 'agent',
  deciding: false,
  buildDisabled: false,
  onDecide: vi.fn(),
  onBuild: vi.fn(),
  onViewPlan: vi.fn(),
  disabled: false,
  pending: false,
  running: false,
  stopping: false,
  onStop: vi.fn(),
  onSubmit: async () => true,
  models: [],
  modelId: null,
  effort: null,
  defaultModelId: 'model',
  defaultEffort: null,
  onModeChange: vi.fn(),
  onModelChange: vi.fn(),
  onEffortChange: vi.fn(),
  draftKey: 's1'
}

describe('ChatTray', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  afterEach(() => {
    cleanup()
  })

  it('shows a closed handle that opens the tray', () => {
    const onOpen = vi.fn()
    render(
      <ChatTray {...chatProps} title="Session" open={false} onOpen={onOpen} onClose={vi.fn()} />
    )

    const handle = screen.getByRole('button', { name: 'Show chat' })
    expect(handle.getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByTestId('chat-panel')).toBeNull()

    fireEvent.click(handle)
    expect(onOpen).toHaveBeenCalledTimes(1)
  })

  it('renders the panel when open and closes with the button or Escape', () => {
    const onClose = vi.fn()
    render(<ChatTray {...chatProps} title="Session" open onOpen={vi.fn()} onClose={onClose} />)

    expect(screen.getByTestId('chat-panel')).toBeTruthy()
    expect(screen.getByText('Session')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Close chat' }))
    expect(onClose).toHaveBeenCalledTimes(1)

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(2)
  })

  it('resizes with the arrow keys and remembers the width', () => {
    render(<ChatTray {...chatProps} title="Session" open onOpen={vi.fn()} onClose={vi.fn()} />)

    const resizer = screen.getByRole('separator', { name: 'Resize chat tray' })
    const start = Number(resizer.getAttribute('aria-valuenow'))

    // ArrowLeft widens; ArrowRight narrows.
    fireEvent.keyDown(resizer, { key: 'ArrowLeft' })
    expect(Number(resizer.getAttribute('aria-valuenow'))).toBeGreaterThan(start)
    fireEvent.keyDown(resizer, { key: 'ArrowRight' })
    expect(Number(resizer.getAttribute('aria-valuenow'))).toBe(start)

    // Home pins to the minimum, End to the maximum.
    fireEvent.keyDown(resizer, { key: 'Home' })
    expect(Number(resizer.getAttribute('aria-valuenow'))).toBe(
      Number(resizer.getAttribute('aria-valuemin'))
    )
    fireEvent.keyDown(resizer, { key: 'End' })
    expect(Number(resizer.getAttribute('aria-valuenow'))).toBe(
      Number(resizer.getAttribute('aria-valuemax'))
    )

    // The chosen width is written so it survives a remount.
    expect(Number(localStorage.getItem('robi.docsTrayWidth'))).toBe(
      Number(resizer.getAttribute('aria-valuemax'))
    )
  })
})
