import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { getDoc, listDocs } from '../../api/docs'
import { useChatStore } from '../../state/chatStore'
import { DocsScreen } from './DocsScreen'

vi.mock('../../api/docs', () => ({
  listDocs: vi.fn(),
  getDoc: vi.fn(),
  searchDocs: vi.fn()
}))

vi.mock('../../api/codeIndex', () => ({
  getIndexStatus: vi.fn()
}))

// The diagram renderer pulls in mermaid; a stub keeps the test light.
vi.mock('../chat/MermaidDiagram', () => ({ MermaidDiagram: () => null }))

const listDocsMock = vi.mocked(listDocs)
const getDocMock = vi.mocked(getDoc)

function renderDocs(workspaceId: string) {
  render(
    <MemoryRouter initialEntries={['/docs?file=docs/a.md']}>
      <DocsScreen workspaceId={workspaceId} />
    </MemoryRouter>
  )
}

describe('DocsScreen refresh on an agent edit', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useChatStore.setState({ activeSessionId: 's1', draftSelected: false, reviewTickBySession: {} })
  })

  afterEach(() => {
    cleanup()
    useChatStore.setState({ activeSessionId: null, reviewTickBySession: {} })
  })

  it('re-fetches the open document when the active session ticks', async () => {
    listDocsMock.mockResolvedValue({ files: [{ path: 'docs/a.md' }] } as never)
    getDocMock.mockResolvedValueOnce({ content: 'first version' } as never)
    getDocMock.mockResolvedValueOnce({ content: 'second version' } as never)

    renderDocs('ws-refresh-doc')
    await screen.findByText('first version')
    expect(getDocMock).toHaveBeenCalledTimes(1)

    act(() => {
      useChatStore.getState().bumpReview('s1')
    })

    await screen.findByText('second version')
    expect(getDocMock).toHaveBeenCalledTimes(2)
  })

  it('re-fetches the tree listing on a tick', async () => {
    listDocsMock.mockResolvedValue({ files: [{ path: 'docs/a.md' }] } as never)
    getDocMock.mockResolvedValue({ content: 'doc' } as never)

    renderDocs('ws-refresh-tree')
    await screen.findByText('doc')
    expect(listDocsMock).toHaveBeenCalledTimes(1)

    act(() => {
      useChatStore.getState().bumpReview('s1')
    })

    await waitFor(() => {
      expect(listDocsMock).toHaveBeenCalledTimes(2)
    })
  })
})
