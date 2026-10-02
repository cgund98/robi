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

  it('shows the last four output lines until the shell card is opened', () => {
    const command = 'make lint && cargo fmt --all -- --check && cargo clippy --workspace'
    const stdout = ['one', 'two', 'three', 'four', 'five', 'six'].join('\n')
    render(
      <ToolCallCard
        call={call({
          name: 'shell',
          args: { command },
          result: { stdout, stderr: '', exit_code: 0, sandboxed: true }
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    const summary = screen.getByRole('button', { name: /Run/ })
    expect(summary.querySelector('[class*="target"]')?.textContent).toBe('make')
    const panel = screen.getByText(/three/).closest('pre')
    expect(panel?.textContent).toBe('three\nfour\nfive\nsix')
    expect(panel?.getAttribute('data-more-above')).toBe('true')
    expect(panel?.getAttribute('data-more-below')).toBe('false')
    expect(screen.queryByText('$')).toBeNull()
    fireEvent.click(summary)
    const opened = screen.getByText('$').closest('pre')
    expect(opened?.textContent).toContain(command)
    expect(opened?.textContent).toContain('one')
    expect(opened?.textContent).toContain('six')
  })

  it('opens a shell approval on the full command', () => {
    const { rerender } = render(
      <ToolCallCard
        call={call({
          name: 'shell',
          execution_status: 'running',
          result: undefined,
          args: { command: 'make lint', unsandboxed: true }
        })}
        phase="responding"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.queryByText('$')).toBeNull()
    rerender(
      <ToolCallCard
        call={call({
          name: 'shell',
          execution_status: 'not_started',
          result: undefined,
          args: { command: 'make lint', unsandboxed: true }
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    const card = screen.getByText('$').closest('pre')?.parentElement
    expect(card?.textContent).toContain('Run unsandboxed')
    expect(card?.textContent).toContain('Approve')
    expect(card?.querySelector('[class*="target"]')?.textContent).toBe('make')
    expect(card?.textContent).toContain('make lint')
  })
})
