import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
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

  it('opens a relative markdown link in the viewer', () => {
    const onDocLink = vi.fn()
    render(
      <AssistantMarkdown
        text={'See [chat](./chat-ui.md) and [web](https://example.com).'}
        docPath="docs/src/design/shell/docs-viewer.md"
        onDocLink={onDocLink}
      />
    )
    fireEvent.click(screen.getByRole('link', { name: 'chat' }))
    expect(onDocLink).toHaveBeenCalledWith('docs/src/design/shell/chat-ui.md')
    const external = screen.getByRole('link', { name: 'web' })
    expect(external.getAttribute('target')).toBe('_blank')
    expect(external.getAttribute('href')).toBe('https://example.com')
  })

  it('keeps a non-mermaid fence as a code block', () => {
    renderMermaidMock.mockClear()
    render(<AssistantMarkdown text={'```json\n{ "a": 1 }\n```'} />)
    expect(screen.getByText('{ "a": 1 }').closest('pre')).toBeTruthy()
    expect(renderMermaidMock).not.toHaveBeenCalled()
  })

  it('stamps document-mode blocks with their raw source lines', () => {
    const source = '# Title\n\nfirst line\nsecond line\n\n- item one\n- item two\n'
    const { container } = render(<AssistantMarkdown text={source} document />)
    // Heading is one line; the paragraph spans its two lines; each list item is
    // its own line. These are file lines, not rendered line counts.
    expect(screen.getByRole('heading', { name: 'Title' }).getAttribute('data-md-lines')).toBe('1')
    expect(container.querySelector('p[data-md-lines="3-4"]')?.textContent).toBe(
      'first line\nsecond line'
    )
    expect(
      Array.from(container.querySelectorAll('li[data-md-lines]')).map((item) =>
        item.getAttribute('data-md-lines')
      )
    ).toEqual(['6', '7'])
  })

  it('leaves chat mode without line stamps', () => {
    const { container } = render(<AssistantMarkdown text={'# Title\n\nbody\n'} />)
    expect(container.querySelector('[data-md-lines]')).toBeNull()
  })
})
