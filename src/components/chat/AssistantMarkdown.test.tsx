import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AssistantMarkdown } from './AssistantMarkdown'
import { renderMermaid } from './mermaid'

vi.mock('./mermaid', () => ({ renderMermaid: vi.fn() }))

const renderMermaidMock = vi.mocked(renderMermaid)

describe('AssistantMarkdown', () => {
  afterEach(() => {
    cleanup()
  })

  it('renders headings, lists, and code instead of the raw marks', () => {
    render(
      <AssistantMarkdown text={'## Plan\n\n- read `src/main.rs`\n\n```rust\nfn main() {}\n```'} />
    )
    expect(screen.getByRole('heading', { name: 'Plan' })).toBeTruthy()
    expect(screen.getByRole('listitem').textContent).toContain('read')
    expect(screen.getByText('src/main.rs').tagName).toBe('CODE')
    expect(screen.getByText('fn main() {}').closest('pre')).toBeTruthy()
    expect(screen.queryByText(/## Plan/)).toBeNull()
  })

  it('renders a mermaid fence as a diagram', async () => {
    renderMermaidMock.mockResolvedValue('<svg><text>graph</text></svg>')
    render(<AssistantMarkdown text={'```mermaid\ngraph TD;\nA-->B;\n```'} />)
    expect(renderMermaidMock).toHaveBeenCalledWith('graph TD;\nA-->B;')
    await waitFor(() => {
      expect(screen.getByRole('img', { name: 'Diagram' })).toBeTruthy()
    })
    expect(screen.getByRole('img', { name: 'Diagram' }).closest('pre')).toBeNull()
  })

  it('keeps a non-mermaid fence as a code block', () => {
    renderMermaidMock.mockClear()
    render(<AssistantMarkdown text={'```json\n{ "a": 1 }\n```'} />)
    expect(screen.getByText('{ "a": 1 }').closest('pre')).toBeTruthy()
    expect(renderMermaidMock).not.toHaveBeenCalled()
  })
})
