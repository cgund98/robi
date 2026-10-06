import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { ChatMessage } from '../../api/messages'
import { Transcript } from './Transcript'
import { distanceFromBottom } from './transcriptFollow'
import type { ChatToolCall } from './toolCallView'

function message(id: string, content: string): ChatMessage {
  return { id, role: 'user', content, tool_calls: [] }
}

function installScrollMetrics(metrics: { scrollHeight: number; clientHeight: number }) {
  const scrollHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'scrollHeight')
  const clientHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight')
  Object.defineProperty(HTMLElement.prototype, 'scrollHeight', {
    configurable: true,
    get() {
      return metrics.scrollHeight
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get() {
      return metrics.clientHeight
    }
  })
  return () => {
    if (scrollHeight) {
      Object.defineProperty(HTMLElement.prototype, 'scrollHeight', scrollHeight)
    }
    if (clientHeight) {
      Object.defineProperty(HTMLElement.prototype, 'clientHeight', clientHeight)
    }
  }
}

describe('Transcript activity dots', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('steps the dots beside Thinking', () => {
    vi.useFakeTimers()
    render(
      <Transcript
        messages={[]}
        echo={null}
        phase="thinking"
        mode="agent"
        deciding={false}
        onDecide={() => {}}
      />
    )
    const dots = document.querySelector('[aria-hidden]')
    expect(screen.getByText('Thinking')).toBeTruthy()
    expect(dots?.textContent).toBe('.')
    act(() => {
      vi.advanceTimersByTime(400)
    })
    expect(dots?.textContent).toBe('..')
    act(() => {
      vi.advanceTimersByTime(400)
    })
    expect(dots?.textContent).toBe('...')
    act(() => {
      vi.advanceTimersByTime(400)
    })
    expect(dots?.textContent).toBe('.')
  })
})

describe('Transcript todos', () => {
  it('shows a finished task in the turn and leaves the open ones at the bottom', () => {
    const plan: ChatToolCall = {
      id: 'plan',
      name: 'write_plan',
      args: {
        body: '# Ship modes\n',
        todos: [
          { id: 'modes', content: 'Add the mode registry', status: 'pending' },
          { id: 'wire', content: 'Register the tool', status: 'in_progress' }
        ]
      },
      approval_status: 'not_required',
      execution_status: 'succeeded',
      result: { path: '.robi/plans/ship-modes.md', status: 'created' }
    }
    const patch: ChatToolCall = {
      id: 'patch',
      name: 'todos',
      args: {
        path: '.robi/plans/ship-modes.md',
        update: [{ id: 'modes', status: 'completed' }]
      },
      approval_status: 'not_required',
      execution_status: 'succeeded',
      result: {
        items: [
          { id: 'modes', content: 'Add the mode registry', status: 'completed' },
          { id: 'wire', content: 'Register the tool', status: 'in_progress' }
        ]
      }
    }
    render(
      <Transcript
        messages={[
          { id: 'user-1', role: 'user', content: 'Build it', tool_calls: [] },
          { id: 'assistant-1', role: 'assistant', content: '', tool_calls: [plan, patch] }
        ]}
        echo={null}
        phase="idle"
        mode="agent"
        deciding={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('Add the mode registry')).toBeTruthy()
    expect(screen.queryByRole('button', { name: /Update/ })).toBeNull()
    const remaining = screen.getByRole('list', { name: 'Remaining tasks' })
    expect(remaining.textContent).toContain('Register the tool')
    expect(remaining.textContent).not.toContain('Add the mode registry')
    expect(remaining.compareDocumentPosition(screen.getByText('Add the mode registry'))).toBe(
      Node.DOCUMENT_POSITION_PRECEDING
    )
  })

  it('leaves the open checklist off while the session is still planning', () => {
    const { container } = render(
      <Transcript
        messages={[
          { id: 'user-1', role: 'user', content: 'Plan the modes', tool_calls: [] },
          {
            id: 'assistant-1',
            role: 'assistant',
            content: '',
            tool_calls: [
              {
                id: 'plan',
                name: 'write_plan',
                args: {
                  body: '# Ship modes\n',
                  todos: [
                    { id: 'modes', content: 'Add the mode registry', status: 'pending' },
                    { id: 'wire', content: 'Register the tool', status: 'pending' }
                  ]
                },
                approval_status: 'not_required',
                execution_status: 'succeeded',
                result: { path: '.robi/plans/ship-modes.md', status: 'created' }
              }
            ]
          }
        ]}
        echo={null}
        phase="idle"
        mode="plan"
        deciding={false}
        onDecide={() => {}}
      />
    )
    expect(container.textContent).toContain('Created Plan')
    expect(container.querySelector('[aria-label="Remaining tasks"]')).toBeNull()
  })
})

describe('Transcript follow', () => {
  it('measures distance from the bottom', () => {
    expect(distanceFromBottom({ scrollHeight: 500, scrollTop: 100, clientHeight: 200 })).toBe(200)
  })

  it('follows the tail when the reader is already at the bottom', () => {
    const metrics = { scrollHeight: 1000, clientHeight: 400 }
    const restore = installScrollMetrics(metrics)
    const props = {
      echo: null,
      phase: 'thinking' as const,
      mode: 'agent' as const,
      deciding: false,
      onDecide: () => {}
    }
    const { container, rerender } = render(
      <Transcript {...props} messages={[message('a', 'first')]} />
    )
    const scroller = container.firstElementChild as HTMLDivElement
    expect(scroller.scrollTop).toBe(600)
    metrics.scrollHeight = 1800
    rerender(<Transcript {...props} messages={[message('a', 'first'), message('b', 'second')]} />)
    expect(scroller.scrollTop).toBe(1400)
    restore()
  })

  it('stays put after a wheel upward when a later row arrives', () => {
    const metrics = { scrollHeight: 1000, clientHeight: 400 }
    const restore = installScrollMetrics(metrics)
    const props = {
      echo: null,
      phase: 'thinking' as const,
      mode: 'agent' as const,
      deciding: false,
      onDecide: () => {}
    }
    const { container, rerender } = render(
      <Transcript {...props} messages={[message('a', 'first')]} />
    )
    const scroller = container.firstElementChild as HTMLDivElement
    expect(scroller.scrollTop).toBe(600)
    scroller.dispatchEvent(new WheelEvent('wheel', { deltaY: -20 }))
    metrics.scrollHeight = 1800
    rerender(<Transcript {...props} messages={[message('a', 'first'), message('b', 'second')]} />)
    expect(scroller.scrollTop).toBe(600)
    restore()
  })
})

describe('Transcript compaction divider', () => {
  it('renders a Context compacted divider for a summary message', () => {
    render(
      <Transcript
        messages={[
          {
            id: 'sum',
            role: 'user',
            content: 'decisions so far',
            tool_calls: [],
            compaction: true
          },
          { id: 'user-1', role: 'user', content: 'keep going', tool_calls: [] }
        ]}
        echo={null}
        phase="idle"
        mode="agent"
        deciding={false}
        onDecide={() => {}}
      />
    )
    const divider = screen.getByRole('separator')
    expect(divider.textContent).toBe('Context compacted')
    // The summary text is not shown as a user bubble.
    expect(screen.queryByText('decisions so far')).toBeNull()
    expect(screen.getByText('keep going')).toBeTruthy()
  })
})

describe('Transcript file chips', () => {
  it('renders a chip for a stored attachment', () => {
    render(
      <Transcript
        messages={[
          {
            id: 'user-1',
            role: 'user',
            content: 'see this',
            tool_calls: [],
            files: [{ name: 'error.rs', start_line: 29, end_line: 34 }]
          }
        ]}
        echo={null}
        phase="idle"
        mode="agent"
        deciding={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('error.rs')).toBeTruthy()
    expect(screen.getByText('(29-34)')).toBeTruthy()
  })

  it('renders the pending echo attachments before the stored row exists', () => {
    render(
      <Transcript
        messages={[]}
        echo="one moment"
        echoFiles={[{ name: 'notes.txt' }]}
        phase="thinking"
        mode="agent"
        deciding={false}
        onDecide={() => {}}
      />
    )
    expect(screen.getByText('notes.txt')).toBeTruthy()
    expect(screen.getByText('one moment')).toBeTruthy()
  })
})
