import { describe, expect, it } from 'vitest'

import type { ChatMessage } from '../../api/messages'
import type { ChatToolCall } from './toolCallView'
import { finishedTodos, remainingTodos } from './todoProgress'

function call(overrides: Partial<ChatToolCall> = {}): ChatToolCall {
  return {
    id: 'call-1',
    name: 'todos',
    args: {},
    approval_status: 'not_required',
    execution_status: 'succeeded',
    ...overrides
  }
}

function assistant(id: string, toolCalls: ChatToolCall[]): ChatMessage {
  return { id, role: 'assistant', content: '', tool_calls: toolCalls }
}

const plan = call({
  id: 'plan',
  name: 'write_plan',
  args: {
    body: '# Ship modes\n',
    todos: [
      { id: 'modes', content: 'Add the mode registry', status: 'pending' },
      { id: 'wire', content: 'Register the tool', status: 'in_progress' },
      { id: 'old', content: 'Already done', status: 'completed' }
    ]
  },
  result: { path: '.robi/plans/ship-modes.md', status: 'created' }
})

describe('finishedTodos', () => {
  it('lists tasks the call marked completed or canceled', () => {
    expect(
      finishedTodos(
        call({
          args: {
            path: '.robi/plans/ship-modes.md',
            update: [
              { id: 'modes', status: 'completed' },
              { id: 'wire', status: 'in_progress' }
            ],
            add: [{ id: 'drop', content: 'Skip the extra pass', status: 'canceled' }]
          },
          result: {
            items: [
              { id: 'modes', content: 'Add the mode registry', status: 'completed' },
              { id: 'wire', content: 'Register the tool', status: 'in_progress' },
              { id: 'drop', content: 'Skip the extra pass', status: 'canceled' }
            ]
          }
        })
      )
    ).toEqual([
      { id: 'modes', content: 'Add the mode registry', status: 'completed' },
      { id: 'drop', content: 'Skip the extra pass', status: 'canceled' }
    ])
  })

  it('leaves a call that only advances a task off the transcript', () => {
    expect(
      finishedTodos(
        call({
          args: { update: [{ id: 'wire', status: 'in_progress' }] },
          result: { items: [{ id: 'wire', content: 'Register the tool', status: 'in_progress' }] }
        })
      )
    ).toEqual([])
  })

  it('ignores a task that was already finished before this call', () => {
    expect(
      finishedTodos(
        call({
          args: { update: [{ id: 'wire', status: 'in_progress' }] },
          result: {
            items: [
              { id: 'modes', content: 'Add the mode registry', status: 'completed' },
              { id: 'wire', content: 'Register the tool', status: 'in_progress' }
            ]
          }
        })
      )
    ).toEqual([])
  })
})

describe('remainingTodos', () => {
  it('keeps open tasks from the plan until a todos call replaces the list', () => {
    const messages = [assistant('a', [plan])]
    expect(remainingTodos(messages).map((item) => item.id)).toEqual(['modes', 'wire'])

    messages.push(
      assistant('b', [
        call({
          id: 'patch',
          args: { update: [{ id: 'modes', status: 'completed' }] },
          result: {
            items: [
              { id: 'modes', content: 'Add the mode registry', status: 'completed' },
              { id: 'wire', content: 'Register the tool', status: 'pending' }
            ]
          }
        })
      ])
    )
    expect(remainingTodos(messages)).toEqual([
      { id: 'wire', content: 'Register the tool', status: 'pending' }
    ])
  })

  it('keeps the previous list when the latest todos call fails', () => {
    expect(
      remainingTodos([
        assistant('a', [plan]),
        assistant('b', [
          call({
            execution_status: 'failed',
            error: 'unknown task missing',
            result: { items: [] }
          })
        ])
      ]).map((item) => item.id)
    ).toEqual(['modes', 'wire'])
  })
})
