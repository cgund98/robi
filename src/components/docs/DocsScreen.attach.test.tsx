import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { getDoc, listDocs } from '../../api/docs'
import { useWorkspaceStore } from '../../state/workspaceStore'
import { base64ToBytes } from '../chat/textAttachments'
import { DocsScreen } from './DocsScreen'

vi.mock('../../api/docs', () => ({
  listDocs: vi.fn(),
  getDoc: vi.fn(),
  searchDocs: vi.fn()
}))

vi.mock('../../api/codeIndex', () => ({
  getIndexStatus: vi.fn()
}))

vi.mock('../chat/MermaidDiagram', () => ({ MermaidDiagram: () => null }))

const listDocsMock = vi.mocked(listDocs)
const getDocMock = vi.mocked(getDoc)

const CONTENT = '# Title\n\nalpha\nbeta\n\n- one\n- two\n'

function renderDocs(onAttachLine?: (file: unknown) => void) {
  listDocsMock.mockResolvedValue({ files: [{ path: 'docs/a.md' }] } as never)
  getDocMock.mockResolvedValue({ content: CONTENT } as never)
  return render(
    <MemoryRouter initialEntries={['/docs?file=docs/a.md']}>
      <DocsScreen workspaceId="ws-1" onAttachLine={onAttachLine} />
    </MemoryRouter>
  )
}

function decode(base64: string): string {
  return new TextDecoder().decode(base64ToBytes(base64))
}

describe('DocsScreen attach a line', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useWorkspaceStore.setState({
      workspaces: [{ id: 'ws-1', name: 'ws', root: '/tmp/ws' }] as never
    })
  })

  afterEach(() => {
    cleanup()
  })

  it('shows the button on hover and attaches the raw line range', async () => {
    const onAttachLine = vi.fn()
    const { container } = renderDocs(onAttachLine)
    await screen.findByRole('heading', { name: 'Title' })

    // No button until a block is hovered.
    expect(screen.queryByRole('button', { name: /Add lines? .* to chat/ })).toBeNull()

    const paragraph = container.querySelector('p[data-md-lines="3-4"]')
    expect(paragraph).toBeTruthy()
    fireEvent.mouseMove(paragraph!)

    const button = screen.getByRole('button', { name: 'Add lines 3 to 4 to chat' })
    fireEvent.click(button)

    expect(onAttachLine).toHaveBeenCalledTimes(1)
    const file = onAttachLine.mock.calls[0][0] as {
      name: string
      path: string
      absolutePath: string
      startLine: number
      endLine: number
      contentBase64: string
    }
    expect(file.name).toBe('a.md')
    expect(file.path).toBe('docs/a.md')
    expect(file.absolutePath).toBe('/tmp/ws/docs/a.md')
    expect(file.startLine).toBe(3)
    expect(file.endLine).toBe(4)
    expect(decode(file.contentBase64)).toBe('alpha\nbeta')
  })

  it('attaches a single list item line', async () => {
    const onAttachLine = vi.fn()
    const { container } = renderDocs(onAttachLine)
    await screen.findByText('one')

    fireEvent.mouseMove(container.querySelector('li[data-md-lines="6"]')!)
    fireEvent.click(screen.getByRole('button', { name: 'Add line 6 to chat' }))

    const file = onAttachLine.mock.calls[0][0] as { startLine: number; contentBase64: string }
    expect(file.startLine).toBe(6)
    expect(decode(file.contentBase64)).toBe('- one')
  })

  it('clears the button when the pointer moves off a block', async () => {
    const { container } = renderDocs(vi.fn())
    await screen.findByRole('heading', { name: 'Title' })

    const paragraph = container.querySelector('p[data-md-lines="3-4"]')!
    fireEvent.mouseMove(paragraph)
    expect(screen.getByRole('button', { name: 'Add lines 3 to 4 to chat' })).toBeTruthy()

    // Moving onto the markdown wrapper (an element with no line stamp) bubbles
    // to the viewer handler, which clears the hover.
    fireEvent.mouseMove(container.querySelector('[data-md-lines]')!.closest('div')!)
    expect(screen.queryByRole('button', { name: /Add lines? .* to chat/ })).toBeNull()
  })

  it('offers no button when there is no composer', async () => {
    const { container } = renderDocs(undefined)
    await screen.findByRole('heading', { name: 'Title' })

    fireEvent.mouseMove(container.querySelector('p[data-md-lines="3-4"]')!)
    expect(screen.queryByRole('button', { name: /Add lines? .* to chat/ })).toBeNull()
  })
})
