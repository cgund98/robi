import type { ChatMessage } from '../../api/messages'
import { parsePlanTodos, type ChatToolCall, type PlanTodo } from './toolCallView'

export type FinishedTodo = {
  id: string
  content: string
  status: 'completed' | 'canceled'
}

/** Tasks a successful `todos` call marked completed or canceled. */
export function finishedTodos(call: ChatToolCall): FinishedTodo[] {
  if (call.name !== 'todos' || call.error || call.execution_status !== 'succeeded') {
    return []
  }
  const marked = markedFinished(call.args)
  if (marked.size === 0) {
    return []
  }
  const finished: FinishedTodo[] = []
  for (const item of parsePlanTodos(record(call.result)?.items)) {
    const status = marked.get(item.id)
    if (!status || item.status !== status) {
      continue
    }
    finished.push({ id: item.id, content: item.content, status })
  }
  return finished
}

/**
 * Pending and in-progress tasks from the latest checklist in the transcript.
 * A later successful `todos` result replaces a `write_plan` list. A failed or
 * still-running call leaves the previous list in place.
 */
export function remainingTodos(messages: ChatMessage[]): PlanTodo[] {
  let current: PlanTodo[] = []
  for (const message of messages) {
    if (message.role !== 'assistant') {
      continue
    }
    for (const call of message.tool_calls) {
      if (call.error || call.execution_status !== 'succeeded') {
        continue
      }
      if (call.name === 'write_plan') {
        current = parsePlanTodos(record(call.args)?.todos)
      } else if (call.name === 'todos') {
        current = parsePlanTodos(record(call.result)?.items)
      }
    }
  }
  return current.filter((item) => item.status === 'pending' || item.status === 'in_progress')
}

function markedFinished(args: unknown): Map<string, FinishedTodo['status']> {
  const fields = record(args)
  const marked = new Map<string, FinishedTodo['status']>()
  for (const key of ['update', 'add'] as const) {
    const list = fields?.[key]
    if (!Array.isArray(list)) {
      continue
    }
    for (const item of list) {
      const row = record(item)
      const id = stringField(row, 'id').trim()
      const status = finishStatus(stringField(row, 'status'))
      if (id && status) {
        marked.set(id, status)
      }
    }
  }
  return marked
}

function finishStatus(value: string): FinishedTodo['status'] | null {
  const status = value.trim()
  if (status === 'completed' || status === 'canceled') {
    return status
  }
  return null
}

function record(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as Record<string, unknown>
  }
  return null
}

function stringField(value: Record<string, unknown> | null, key: string): string {
  const field = value?.[key]
  return typeof field === 'string' ? field : ''
}
