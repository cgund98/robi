import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { getDoc, listDocs } from '../../api/docs'
import { DocsScreen } from './DocsScreen'

vi.mock('../../api/docs', () => ({
  listDocs: vi.fn(),
  getDoc: vi.fn(),
  searchDocs: vi.fn()
}))

vi.mock('../../api/codeIndex', () => ({
  getIndexStatus: vi.fn()
}))

// The diagram renderer pulls in mermaid; a stub keeps the test light. It never
// renders here — the document is plain prose.
vi.mock('../chat/MermaidDiagram', () => ({ MermaidDiagram: () => null }))

const listDocsMock = vi.mocked(listDocs)
const getDocMock = vi.mocked(getDoc)

function renderDocs(content: string) {
  listDocsMock.mockResolvedValue({ files: [{ path: 'docs/a.md' }] } as never)
  getDocMock.mockResolvedValue({ content } as never)
  render(
    <MemoryRouter initialEntries={['/docs?file=docs/a.md']}>
      <DocsScreen workspaceId="ws-1" />
    </MemoryRouter>
  )
}

async function openFindBar() {
  // The platform check decides which modifier counts; setting both makes the
  // shortcut fire on either branch.
  fireEvent.keyDown(window, { key: 'f', ctrlKey: true, metaKey: true })
  return screen.findByRole('textbox', { name: 'Find in document' })
}

describe('DocsScreen find', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  afterEach(() => {
    cleanup()
  })

  it('opens with the shortcut, counts matches, steps, and closes on Escape', async () => {
    renderDocs('alpha beta alpha')
    await screen.findByText('alpha beta alpha')

    const field = await openFindBar()
    fireEvent.change(field, { target: { value: 'alpha' } })

    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe('1 of 2')
    })

    fireEvent.keyDown(field, { key: 'Enter' })
    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe('2 of 2')
    })

    // Wrap-around from the last match returns to the first.
    fireEvent.click(screen.getByRole('button', { name: 'Next match' }))
    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe('1 of 2')
    })

    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => {
      expect(screen.queryByRole('textbox', { name: 'Find in document' })).toBeNull()
    })
  })

  it('narrows to case-sensitive matches when the toggle is on', async () => {
    renderDocs('Alpha alpha')
    await screen.findByText('Alpha alpha')

    const field = await openFindBar()
    fireEvent.change(field, { target: { value: 'Alpha' } })

    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe('1 of 2')
    })

    fireEvent.click(screen.getByRole('button', { name: 'Match case' }))
    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe('1 of 1')
    })
  })

  it('recomputes when another document is opened', async () => {
    listDocsMock.mockResolvedValue({
      files: [{ path: 'docs/a.md' }, { path: 'docs/b.md' }]
    } as never)
    getDocMock.mockImplementation(((_workspace: string, path: string) =>
      Promise.resolve({
        content: path === 'docs/b.md' ? 'beta beta beta' : 'alpha alpha'
      })) as never)
    render(
      <MemoryRouter initialEntries={['/docs?file=docs/a.md']}>
        <DocsScreen workspaceId="ws-1" />
      </MemoryRouter>
    )

    await screen.findByText('alpha alpha')
    const field = await openFindBar()
    fireEvent.change(field, { target: { value: 'alpha' } })
    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe('1 of 2')
    })

    fireEvent.click(screen.getByRole('button', { name: 'b.md' }))
    await screen.findByText('beta beta beta')
    // The query is unchanged, so the new document is searched for it.
    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe('No results')
    })
  })
})
