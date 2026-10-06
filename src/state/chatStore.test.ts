import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('../api/messages', () => ({
  listMessages: vi.fn(async () => []),
  decideToolCall: vi.fn(async () => {}),
  stopSession: vi.fn(async () => {}),
  submitInstruction: vi.fn(async () => {}),
  getMessage: vi.fn(),
  getToolOriginal: vi.fn(),
  imageUrl: (sessionId: string, imageId: string) =>
    `/api/v1/chat_sessions/${sessionId}/images/${imageId}`
}))

vi.mock('../api/sessions', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/sessions')>()
  return {
    ...actual,
    listSessions: vi.fn(async () => []),
    getSession: vi.fn(async (id: string) => ({
      id,
      workspace_id: 'w',
      title: null,
      path_allow_read: [],
      path_allow_write: [],
      path_deny_read: [],
      path_deny_write: [],
      allow_hosts: [],
      mode: 'agent',
      model_config: {},
      created_at: '',
      updated_at: '',
      last_used_at: '',
      has_pending_agent: false,
      turn_display: 'idle'
    }))
  }
})

import { decideToolCall, getMessage } from '../api/messages'
import { chat, patchChat } from './chatStore'

describe('decideCall', () => {
  afterEach(() => {
    patchChat({
      busy: false,
      error: null,
      sessions: [],
      messagesBySession: {},
      phaseBySession: {}
    })
    vi.mocked(decideToolCall).mockReset()
    vi.mocked(getMessage).mockReset()
  })

  it('reloads the message when the decision fails and keeps the bar if the call is still open', async () => {
    vi.mocked(decideToolCall).mockRejectedValueOnce(new Error('chat session is running'))
    vi.mocked(getMessage).mockResolvedValueOnce({
      id: 'm1',
      role: 'assistant',
      text: '',
      tool_calls: [
        {
          id: 'c1',
          name: 'shell',
          arguments: {},
          approval_status: 'pending',
          execution_status: 'not_started'
        }
      ]
    } as never)
    patchChat({
      sessions: [{ id: 's1', has_pending_agent: false } as never],
      messagesBySession: {
        s1: [
          {
            id: 'm1',
            role: 'assistant',
            text: '',
            tool_calls: [
              {
                id: 'c1',
                name: 'shell',
                arguments: {},
                approval_status: 'pending',
                execution_status: 'not_started'
              }
            ]
          } as never
        ]
      }
    })

    await chat.decideCall('s1', 'c1', 'approve')

    expect(getMessage).toHaveBeenCalledWith('s1', 'm1')
    expect(chat.phaseBySession.s1).toBe('idle')
    expect(chat.error).toBe('chat session is running')
    expect(chat.sessions[0]?.has_pending_agent).toBe(false)
  })

  it('keeps thinking when the reloaded call is no longer waiting', async () => {
    vi.mocked(decideToolCall).mockRejectedValueOnce(new Error('chat session is running'))
    vi.mocked(getMessage).mockResolvedValueOnce({
      id: 'm1',
      role: 'assistant',
      text: '',
      tool_calls: [
        {
          id: 'c1',
          name: 'shell',
          arguments: {},
          approval_status: 'approved',
          execution_status: 'running'
        }
      ]
    } as never)
    patchChat({
      messagesBySession: {
        s1: [
          {
            id: 'm1',
            role: 'assistant',
            text: '',
            tool_calls: [
              {
                id: 'c1',
                name: 'shell',
                arguments: {},
                approval_status: 'pending',
                execution_status: 'not_started'
              }
            ]
          } as never
        ]
      }
    })

    await chat.decideCall('s1', 'c1', 'reject')

    expect(chat.phaseBySession.s1).toBe('thinking')
    expect(chat.error).toBeNull()
    expect(chat.messagesBySession.s1?.[0]?.tool_calls[0]?.approval_status).toBe('approved')
  })
})

describe('selectSession', () => {
  afterEach(() => {
    patchChat({
      activeSessionId: null,
      draftSelected: false,
      messagesBySession: {},
      reviewTickBySession: {}
    })
  })

  it('reloads the review so the strip reflects the opened session', async () => {
    await chat.selectSession('s1')

    expect(chat.activeSessionId).toBe('s1')
    expect(chat.reviewTickBySession.s1).toBe(1)
  })

  it('does not reload the review when the session is already open', async () => {
    await chat.selectSession('s1')
    await chat.selectSession('s1')

    expect(chat.reviewTickBySession.s1).toBe(1)
  })
})
