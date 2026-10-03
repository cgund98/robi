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

  it('shows the requested line window on a read', () => {
    render(
      <ToolCallCard
        call={call({ args: { path: 'chat_runtime.rs', offset: 240, limit: 60 } })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('L240-299')).toBeTruthy()
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

  it('keeps a shell command as a row until it is opened', () => {
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
    expect(screen.queryByText('six')).toBeNull()
    expect(screen.queryByText('$')).toBeNull()
    fireEvent.click(summary)
    const opened = screen.getByText('$').closest('pre')
    expect(opened?.textContent).toContain(command)
    expect(opened?.textContent).toContain('one')
    expect(opened?.textContent).toContain('six')
  })

  it('shows the full web search query on the approval bar', () => {
    const query = 'current stable rust version and the 1.90 release notes'
    render(
      <ToolCallCard
        call={call({
          name: 'web_search',
          execution_status: 'not_started',
          result: undefined,
          args: { query }
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('Search')).toBeTruthy()
    expect(screen.getByText('the web')).toBeTruthy()
    expect(document.querySelector('circle')).toBeTruthy()
    expect(screen.getByText(query)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeTruthy()
  })

  it('shows the full URL on a web fetch approval', () => {
    const url = 'https://doc.rust-lang.org/book/ch01-00-getting-started.html'
    render(
      <ToolCallCard
        call={call({
          name: 'web_fetch',
          execution_status: 'not_started',
          result: undefined,
          args: { url }
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('Fetch')).toBeTruthy()
    expect(screen.getByText('doc.rust-lang.org')).toBeTruthy()
    expect(screen.getByText(url)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeTruthy()
  })

  it('opens an MCP approval on the full arguments', () => {
    const args = { issue: 'ENG-1', includeArchived: true }
    render(
      <ToolCallCard
        call={call({
          name: 'mcp_linear_get_issue',
          execution_status: 'not_started',
          result: undefined,
          args
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('MCP call')).toBeTruthy()
    expect(screen.getByText('linear_get_issue')).toBeTruthy()
    expect(screen.getByText(/ENG-1/)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeTruthy()
  })

  it('collapses a finished MCP result', () => {
    const args = { issue: 'ENG-1' }
    const { rerender } = render(
      <ToolCallCard
        call={call({
          name: 'mcp_linear_get_issue',
          approval_status: 'pending',
          execution_status: 'not_started',
          result: undefined,
          args
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText(/ENG-1/)).toBeTruthy()
    rerender(
      <ToolCallCard
        call={call({
          name: 'mcp_linear_get_issue',
          approval_status: 'approved',
          execution_status: 'succeeded',
          result: 'done',
          args
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.queryByText(/ENG-1/)).toBeNull()
    expect(screen.queryByText('done')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /MCP call/ }))
    expect(screen.getByText(/ENG-1/)).toBeTruthy()
    expect(screen.getByText(/done/)).toBeTruthy()
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

  it('lists explore steps while the child runs and keeps them after it finishes', () => {
    const subagent = {
      mode: 'explore',
      description: 'Find resume',
      started_ms: Date.now() - 12_000,
      steps: [{ name: 'grep', target: 'resume', status: 'running' }]
    }
    const { rerender } = render(
      <ToolCallCard
        call={call({
          name: 'delegate',
          execution_status: 'running',
          result: undefined,
          args: { task: 'Where is resume?', mode: 'explore', description: 'Find resume' },
          subagent
        })}
        phase="thinking"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByRole('button', { name: 'Exploring 1 search' })).toBeTruthy()
    expect(screen.queryByLabelText('Running')).toBeNull()
    expect(screen.queryByText('Grepped')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Exploring 1 search' }))
    expect(screen.getByText('Grepped')).toBeTruthy()
    expect(screen.getByText('resume')).toBeTruthy()

    rerender(
      <ToolCallCard
        call={call({
          name: 'delegate',
          execution_status: 'succeeded',
          args: { task: 'Where is resume?', mode: 'explore', description: 'Find resume' },
          result: { mode: 'explore', answer: 'note.txt:1', tool_calls: 1, denied: [] },
          subagent: {
            ...subagent,
            steps: [{ name: 'grep', target: 'resume', status: 'ok' }]
          }
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('Grepped')).toBeTruthy()
    expect(screen.queryByText('note.txt:1')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Answer' }))
    expect(screen.getByText('note.txt:1')).toBeTruthy()
  })

  it('opens a saved plan again and builds from the card', () => {
    const onBuild = vi.fn()
    const onViewPlan = vi.fn()
    render(
      <ToolCallCard
        call={call({
          name: 'write_plan',
          args: {
            plan_name: 'Ship modes',
            body: '# Ship modes\n\nAdd ask, plan, and agent.\n',
            todos: [{ id: 'modes', content: 'Add the mode registry', status: 'pending' }]
          },
          result: { path: '.robi/plans/ship-modes.md', status: 'created' }
        })}
        phase="idle"
        busy={false}
        buildDisabled={false}
        onBuild={onBuild}
        onViewPlan={onViewPlan}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('Created Plan')).toBeTruthy()
    expect(screen.getByText('Add ask, plan, and agent.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'View Plan' }))
    expect(onViewPlan).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Ship modes',
        path: '.robi/plans/ship-modes.md',
        todos: [{ id: 'modes', content: 'Add the mode registry', status: 'pending' }]
      })
    )
    expect(screen.queryByRole('dialog')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Build' }))
    expect(onBuild).toHaveBeenCalledWith('.robi/plans/ship-modes.md')
  })

  it('disables Build while the session is busy', () => {
    render(
      <ToolCallCard
        call={call({
          name: 'write_plan',
          args: { body: '# Ship modes\n\nAdd the modes.\n' },
          result: { path: '.robi/plans/ship-modes.md' }
        })}
        phase="idle"
        busy={false}
        buildDisabled
        onDecide={() => {}}
      />
    )
    expect(screen.getByRole('button', { name: 'Build' }).hasAttribute('disabled')).toBe(true)
  })

  it('keeps a failed plan write on the error row', () => {
    render(
      <ToolCallCard
        call={call({
          name: 'write_plan',
          execution_status: 'failed',
          error: 'plan file does not exist',
          args: { plan_name: 'Ship modes', body: '# Ship modes\n' },
          result: undefined
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.queryByRole('button', { name: 'View Plan' })).toBeNull()
    expect(screen.getByText('Plan')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Plan/ }))
    expect(screen.getByText('plan file does not exist')).toBeTruthy()
  })

  it('opens a retrieve page with its stream, exit, and line numbers', () => {
    render(
      <ToolCallCard
        call={call({
          name: 'retrieve',
          args: { id: '018f3b2c-7c1a-7a21-8c4e-9a21c4e8b0d1', stream: 'stderr' },
          result: {
            stdout: '',
            stderr: '<<<ROBI_LOG omitted=4 lines=1-4>>>\nassertion failed',
            exit_code: 1,
            truncated: false,
            start_line: 40,
            end_line: 41,
            total_lines: 900
          }
        })}
        phase="idle"
        busy={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('018f3b2c stderr')).toBeTruthy()
    expect(screen.getByText('L40-41')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Retrieved/ }))
    expect(screen.getByText('stderr · exit 1 · lines 40–41 of 900')).toBeTruthy()
    expect(screen.getByText('40')).toBeTruthy()
    expect(screen.getByText('assertion failed')).toBeTruthy()
  })
})
