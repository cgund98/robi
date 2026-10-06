import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation, useNavigate } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useChatStore } from '../../state/chatStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import { AppLayout } from './AppLayout'

// Each chat has its own route, so the shell's back and forward controls walk
// between chats. These tests pin the two directions: a click pushes the
// session's route and selects it, and a POP selects whatever the URL names.
vi.mock('../../app/useAgentEventsSSE', () => ({ useAgentEventsSSE: () => {} }))
vi.mock('../../app/approvalNotice', () => ({
  useApprovalNoticeOpen: () => {},
  postApprovalNotice: vi.fn(),
  postTurnFailedNotice: vi.fn(),
  releaseApprovalPause: vi.fn()
}))

vi.mock('../../api/workspaces', () => ({
  listWorkspaces: vi.fn().mockResolvedValue([{ id: 'ws-1', name: 'ws', root: '/tmp/ws' }]),
  createWorkspace: vi.fn(),
  deleteWorkspace: vi.fn()
}))

vi.mock('../../api/mcp', () => ({ focusMcp: vi.fn() }))

const sessions = vi.hoisted(() => [
  { id: 's1', title: 'One', mode: 'agent', model_config: {} },
  { id: 's2', title: 'Two', mode: 'agent', model_config: {} }
])

vi.mock('../../api/sessions', () => ({
  ApiError: class ApiError extends Error {},
  listSessions: vi.fn().mockResolvedValue(sessions),
  createSession: vi.fn(),
  getSession: vi.fn((id: string) => Promise.resolve(sessions.find((s) => s.id === id))),
  updateSession: vi.fn(),
  patchSession: vi.fn(),
  deleteSession: vi.fn(),
  sessionDisplayTitle: (session: { title?: string } | null) => session?.title ?? 'New session',
  errorMessage: (_error: unknown, fallback: string) => fallback,
  statusOf: () => 0
}))

vi.mock('../../api/messages', () => ({
  getMessage: vi.fn(),
  listMessages: vi.fn().mockResolvedValue([]),
  submitInstruction: vi.fn(),
  stopSession: vi.fn(),
  compactSession: vi.fn(),
  decideToolCall: vi.fn(),
  imageUrl: (session: string, image: string) => `/img/${session}/${image}`
}))

vi.mock('../../api/models', () => ({
  listModels: vi.fn().mockResolvedValue([]),
  modelDisplayName: (id: string) => id
}))

vi.mock('../../api/settings', () => ({
  SETTING_KEYS: {
    model: 'model',
    reasoningEffort: 'reasoning_effort',
    modelAsk: 'model.ask',
    reasoningEffortAsk: 'reasoning_effort.ask',
    modelPlan: 'model.plan',
    reasoningEffortPlan: 'reasoning_effort.plan',
    modelAgent: 'model.agent',
    reasoningEffortAgent: 'reasoning_effort.agent'
  },
  getSettings: vi.fn().mockResolvedValue([]),
  getSetting: vi.fn(),
  putSetting: vi.fn(),
  deleteSetting: vi.fn()
}))

vi.mock('./Sidebar', () => ({
  Sidebar: ({
    onSelectSession,
    onNewSession
  }: {
    onSelectSession: (id: string) => void
    onNewSession: () => void
  }) => (
    <div>
      <button data-testid="pick-s1" onClick={() => onSelectSession('s1')}>
        s1
      </button>
      <button data-testid="pick-s2" onClick={() => onSelectSession('s2')}>
        s2
      </button>
      <button data-testid="new-chat" onClick={onNewSession}>
        new
      </button>
    </div>
  )
}))
vi.mock('../chat/ChatHeader', () => ({ ChatHeader: () => null }))
vi.mock('../chat/ChatPanel', () => ({ ChatPanel: () => <div data-testid="chat-panel" /> }))
vi.mock('../chat/ChatTray', () => ({ ChatTray: () => null }))
vi.mock('../chat/Composer', () => ({ Composer: () => null }))
vi.mock('../chat/EmptyGreeting', () => ({ EmptyGreeting: () => null }))
vi.mock('../chat/PlanPage', () => ({ PlanPage: () => null }))
vi.mock('../chat/RenameSessionDialog', () => ({ RenameSessionDialog: () => null }))
vi.mock('../chat/DeleteSessionDialog', () => ({ DeleteSessionDialog: () => null }))
vi.mock('../docs/DocsScreen', () => ({ DocsScreen: () => null }))
vi.mock('../review/ReviewScreen', () => ({ ReviewScreen: () => null }))
vi.mock('./ErrorNotices', () => ({ ErrorNotices: () => null }))

function LocationProbe() {
  const { pathname } = useLocation()
  return <span data-testid="pathname">{pathname}</span>
}

function BackButton() {
  const navigate = useNavigate()
  return (
    <button data-testid="back" onClick={() => navigate(-1)}>
      back
    </button>
  )
}

function renderLayout() {
  render(
    <MemoryRouter initialEntries={['/']}>
      <AppLayout />
      <LocationProbe />
      <BackButton />
    </MemoryRouter>
  )
}

describe('AppLayout chat routing', () => {
  beforeEach(() => {
    useWorkspaceStore.setState({
      loaded: true,
      activeWorkspaceId: 'ws-1',
      workspaces: [{ id: 'ws-1', name: 'ws', root: '/tmp/ws' }] as never,
      error: null
    })
    useChatStore.setState({
      activeSessionId: null,
      draftSelected: false,
      sessions: [],
      messagesBySession: {},
      phaseBySession: {},
      reviewTickBySession: {},
      loading: true
    })
  })

  afterEach(() => {
    cleanup()
    useChatStore.setState({
      activeSessionId: null,
      draftSelected: true,
      sessions: [],
      messagesBySession: {},
      phaseBySession: {},
      reviewTickBySession: {},
      loading: true
    })
  })

  it('opens the most recent session on its own route after load', async () => {
    renderLayout()
    await waitFor(() => expect(screen.getByTestId('pathname').textContent).toBe('/sessions/s1'))
    expect(useChatStore.getState().activeSessionId).toBe('s1')
  })

  it('pushes a route when a session is chosen', async () => {
    renderLayout()
    await waitFor(() => expect(screen.getByTestId('pathname').textContent).toBe('/sessions/s1'))

    fireEvent.click(screen.getByTestId('pick-s2'))

    await waitFor(() => expect(screen.getByTestId('pathname').textContent).toBe('/sessions/s2'))
    expect(useChatStore.getState().activeSessionId).toBe('s2')
  })

  it('switches chats on back navigation', async () => {
    renderLayout()
    await waitFor(() => expect(screen.getByTestId('pathname').textContent).toBe('/sessions/s1'))

    fireEvent.click(screen.getByTestId('pick-s2'))
    await waitFor(() => expect(useChatStore.getState().activeSessionId).toBe('s2'))

    fireEvent.click(screen.getByTestId('back'))

    await waitFor(() => expect(screen.getByTestId('pathname').textContent).toBe('/sessions/s1'))
    expect(useChatStore.getState().activeSessionId).toBe('s1')
  })

  it('returns to the draft route on New chat', async () => {
    renderLayout()
    await waitFor(() => expect(screen.getByTestId('pathname').textContent).toBe('/sessions/s1'))

    fireEvent.click(screen.getByTestId('new-chat'))

    await waitFor(() => expect(screen.getByTestId('pathname').textContent).toBe('/'))
    expect(useChatStore.getState().draftSelected).toBe(true)
  })
})
