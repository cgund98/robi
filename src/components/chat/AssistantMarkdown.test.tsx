import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { AssistantMarkdown } from './AssistantMarkdown'

describe('AssistantMarkdown', () => {
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
})
