import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useChatStore } from '../../state/chatStore'
import { takeComposerAttachments } from '../../state/composerAttachments'
import { useWorkspaceStore } from '../../state/workspaceStore'
import { AppLayout } from './AppLayout'

// The docs view keeps the chat in a tray. These tests pin that choosing a
// session while the docs are open stays on /docs and opens the tray, and that
// the tray starts closed.
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

vi.mock('../../api/sessions', () => ({
  ApiError: class ApiError extends Error {},
  listSessions: vi.fn().mockResolvedValue([]),
  createSession: vi.fn(),
  getSession: vi
    .fn()
    .mockResolvedValue({ id: 's1', title: 'One', mode: 'agent', model_config: {} }),
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

// Leaf components are stubbed so the test is about AppLayout's wiring, not the
// chat surface. Sidebar exposes the session callbacks AppLayout passes down.
vi.mock('./Sidebar', () => ({
  Sidebar: ({
    onSelectSession,
    onNewSession
  }: {
    onSelectSession: (id: string) => void
    onNewSession: () => void
  }) => (
    <div>
      <button data-testid="pick-session" onClick={() => onSelectSession('s1')}>
        pick
      </button>
      <button data-testid="new-chat" onClick={onNewSession}>
        new
      </button>
    </div>
  )
}))
vi.mock('../chat/ChatHeader', () => ({ ChatHeader: () => null }))
vi.mock('../chat/ChatPanel', () => ({ ChatPanel: () => <div data-testid="chat-panel" /> }))
vi.mock('../chat/ChatTray', () => ({
  ChatTray: ({ open, onOpen }: { open: boolean; onOpen: () => void }) =>
    open ? (
      <div data-testid="tray-open" />
    ) : (
      <button data-testid="tray-handle" onClick={onOpen}>
        show
      </button>
    )
}))
vi.mock('../chat/Composer', () => ({ Composer: () => null }))
vi.mock('../chat/EmptyGreeting', () => ({ EmptyGreeting: () => null }))
vi.mock('../chat/PlanPage', () => ({ PlanPage: () => null }))
vi.mock('../chat/RenameSessionDialog', () => ({ RenameSessionDialog: () => null }))
vi.mock('../chat/DeleteSessionDialog', () => ({ DeleteSessionDialog: () => null }))
vi.mock('../docs/DocsScreen', () => ({
  DocsScreen: ({ onAttachLine }: { onAttachLine?: (file: unknown) => void }) => (
    <div data-testid="docs-screen">
      <button
        data-testid="attach-line"
        onClick={() =>
          onAttachLine?.({ name: 'guide.md', contentBase64: '', size: 0, path: 'docs/a.md' })
        }
      >
        attach
      </button>
    </div>
  )
}))
vi.mock('../review/ReviewScreen', () => ({ ReviewScreen: () => null }))
vi.mock('./ErrorNotices', () => ({ ErrorNotices: () => null }))

function LocationProbe() {
  const { pathname } = useLocation()
  return <span data-testid="pathname">{pathname}</span>
}

function renderLayout() {
  render(
    <MemoryRouter initialEntries={['/docs']}>
      <AppLayout />
      <LocationProbe />
    </MemoryRouter>
  )
}

describe('AppLayout docs chat tray', () => {
  beforeEach(() => {
    useWorkspaceStore.setState({
      loaded: true,
      activeWorkspaceId: 'ws-1',
      workspaces: [{ id: 'ws-1', name: 'ws', root: '/tmp/ws' }] as never,
      error: null
    })
  })

  afterEach(() => {
    cleanup()
    takeComposerAttachments('draft')
    useChatStore.setState({
      activeSessionId: null,
      draftSelected: true,
      sessions: [],
      messagesBySession: {},
      phaseBySession: {},
      reviewTickBySession: {}
    })
  })

  it('starts with the tray closed on the docs route', async () => {
    renderLayout()
    await screen.findByTestId('docs-screen')
    expect(screen.getByTestId('tray-handle')).toBeTruthy()
    expect(screen.queryByTestId('tray-open')).toBeNull()
  })

  it('keeps the docs route and opens the tray when a session is chosen', async () => {
    renderLayout()
    await screen.findByTestId('docs-screen')

    fireEvent.click(screen.getByTestId('pick-session'))

    await screen.findByTestId('tray-open')
    expect(screen.getByTestId('pathname').textContent).toBe('/docs')
  })

  it('keeps the docs route and opens the tray on New chat', async () => {
    renderLayout()
    await screen.findByTestId('docs-screen')

    fireEvent.click(screen.getByTestId('new-chat'))

    await screen.findByTestId('tray-open')
    expect(screen.getByTestId('pathname').textContent).toBe('/docs')
  })

  it('opens the tray when a document line is attached', async () => {
    renderLayout()
    await screen.findByTestId('docs-screen')

    fireEvent.click(screen.getByTestId('attach-line'))

    await screen.findByTestId('tray-open')
    expect(screen.getByTestId('pathname').textContent).toBe('/docs')
  })
})
