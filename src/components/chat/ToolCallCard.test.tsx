import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

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
  afterEach(() => {
    cleanup()
  })

  it('shows a finished read and opens the file text', () => {
    render(<ToolCallCard call={call()} phase="idle" busy={false} onDecide={() => {}} />)
    expect(screen.getByText('Read')).toBeTruthy()
    expect(screen.getByText('src/main.rs')).toBeTruthy()
    expect(screen.queryByLabelText('Done')).toBeNull()
    expect(screen.queryByText('fn main() {}')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Read/ }))
    expect(screen.getByText('fn main() {}')).toBeTruthy()
  })

  it('offers Reject and Approve only while the call is waiting', () => {
    const onDecide = vi.fn()
    const waiting = call({
      execution_status: 'not_started',
      result: undefined
    })
    const { rerender } = render(
      <ToolCallCard call={waiting} phase="idle" busy={false} onDecide={onDecide} />
    )
    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))
    fireEvent.click(screen.getByRole('button', { name: 'Reject' }))
    expect(onDecide).toHaveBeenCalledWith('approve')
    expect(onDecide).toHaveBeenCalledWith('reject')

    rerender(<ToolCallCard call={waiting} phase="thinking" busy={false} onDecide={onDecide} />)
    expect(screen.queryByRole('button', { name: 'Approve' })).toBeNull()
    expect(screen.getByLabelText('Running')).toBeTruthy()
  })

  it('shows the proposed edit diff on the approval card', () => {
    const onDecide = vi.fn()
    const lines = ['keep', 'old', 'tail', 'more', 'rest', 'last']
    render(
      <ToolCallCard
        call={call({
          name: 'edit_file',
          execution_status: 'not_started',
          result: undefined,
          args: {
            path: 'src/Transcript.module.css',
            old: lines.join('\n'),
            new: ['keep', 'new', 'tail', 'more', 'rest', 'last'].join('\n')
          }
        })}
        phase="idle"
        busy={false}
        onDecide={onDecide}
      />
    )
    expect(screen.getByText('src/Transcript.module.css')).toBeTruthy()
    expect(screen.getByText('+1')).toBeTruthy()
    expect(screen.getByText('-1')).toBeTruthy()
    expect(screen.getByText('old')).toBeTruthy()
    expect(screen.queryByText('last')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Transcript\.module\.css/ }))
    expect(screen.getByText('last')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))
    fireEvent.click(screen.getByRole('button', { name: 'Reject' }))
    expect(onDecide).toHaveBeenCalledWith('approve')
    expect(onDecide).toHaveBeenCalledWith('reject')
  })

  it('opens a short diff for an edit and leaves the rest closed', () => {
    const patch = ['@@ -1,2 +1,2 @@', ' keep', '-old', '+new', ...Array(30).fill('+extra')].join(
      '\n'
    )
    render(
      <ToolCallCard
        call={call({
          name: 'edit_file',
          args: { path: 'src/replace.rs' },
          result: { path: 'src/replace.rs', additions: 31, deletions: 1, patch }
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('src/replace.rs')).toBeTruthy()
    expect(screen.getByText('+31')).toBeTruthy()
    expect(screen.getByText('-1')).toBeTruthy()
    expect(screen.getByText('old')).toBeTruthy()
    expect(screen.getAllByText('extra')).toHaveLength(1)
    expect(screen.queryByText('9 more lines')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /replace\.rs/ }))
    expect(screen.getByText('old')).toBeTruthy()
    expect(screen.getByText('new')).toBeTruthy()
    expect(screen.getAllByText('extra')).toHaveLength(21)
    expect(screen.getByText('9 more lines')).toBeTruthy()
  })
})
