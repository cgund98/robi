import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { MermaidDiagram } from './MermaidDiagram'
import { renderMermaid } from './mermaid'

vi.mock('./mermaid', () => ({ renderMermaid: vi.fn() }))

const renderMermaidMock = vi.mocked(renderMermaid)

describe('MermaidDiagram', () => {
  afterEach(() => {
    cleanup()
  })

  it('shows the source fence until the SVG is ready, then swaps it in', async () => {
    renderMermaidMock.mockResolvedValue('<svg><circle r="1" /></svg>')
    render(<MermaidDiagram source={'graph TD; A-->B;'} />)
    expect(screen.getByText('graph TD; A-->B;').closest('pre')).toBeTruthy()
    expect(screen.queryByRole('img')).toBeNull()
    await waitFor(() => {
      expect(screen.getByRole('img', { name: 'Diagram' })).toBeTruthy()
    })
  })

  it('keeps the source fence when mermaid rejects the source', async () => {
    renderMermaidMock.mockRejectedValue(new Error('parse error'))
    render(<MermaidDiagram source={'not a diagram'} />)
    await waitFor(() => {
      expect(renderMermaidMock).toHaveBeenCalledWith('not a diagram')
    })
    expect(screen.getByText('not a diagram').closest('pre')).toBeTruthy()
    expect(screen.queryByRole('img')).toBeNull()
  })

  it('does not update after unmount', async () => {
    let resolveRender: (svg: string) => void = () => {}
    renderMermaidMock.mockImplementation(
      () =>
        new Promise<string>((resolve) => {
          resolveRender = resolve
        })
    )
    const { unmount } = render(<MermaidDiagram source={'graph TD; A-->B;'} />)
    unmount()
    resolveRender('<svg />')
    await Promise.resolve()
    expect(screen.queryByRole('img')).toBeNull()
  })
})
