import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ToolCallCard } from './ToolCallCard'
import type { ChatToolCall } from './toolCallView'

function call(overrides: Partial<ChatToolCall> = {}): ChatToolCall {
  return {
    id: 'call-1',
    name: 'read_file',
    args: { path: 'src/main.rs' },
    approval_status: 'pending',
    execution_status: 'succeeded',
    result: { content: 'fn main() {}', start_line: 1 },
    ...overrides
  }
}

describe('ToolCallCard', () => {
  it('shows a finished read and opens the file text', () => {
    render(<ToolCallCard call={call()} phase="idle" busy={false} onDecide={() => {}} />)
    expect(screen.getByText('Read')).toBeTruthy()
    expect(screen.getByText('src/main.rs')).toBeTruthy()
    expect(screen.queryByLabelText('Done')).toBeNull()
    expect(screen.queryByText('fn main() {}')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Read/ }))
    expect(screen.getByText('fn main() {}')).toBeTruthy()
  })

  it('offers Reject and Run only while the call is waiting', () => {
    const onDecide = vi.fn()
    const waiting = call({
      execution_status: 'not_started',
      result: undefined
    })
    const { rerender } = render(
      <ToolCallCard call={waiting} phase="idle" busy={false} onDecide={onDecide} />
    )
    fireEvent.click(screen.getByRole('button', { name: 'Run' }))
    fireEvent.click(screen.getByRole('button', { name: 'Reject' }))
    expect(onDecide).toHaveBeenCalledWith('approve')
    expect(onDecide).toHaveBeenCalledWith('reject')

    rerender(<ToolCallCard call={waiting} phase="thinking" busy={false} onDecide={onDecide} />)
    expect(screen.queryByRole('button', { name: 'Run' })).toBeNull()
    expect(screen.getByLabelText('Running')).toBeTruthy()
  })
})
