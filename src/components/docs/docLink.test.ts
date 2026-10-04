import { describe, expect, it } from 'vitest'

import { resolveMarkdownLink } from './docLink'

describe('resolveMarkdownLink', () => {
  const from = 'docs/src/design/shell/docs-viewer.md'

  it('resolves a sibling and a parent markdown link', () => {
    expect(resolveMarkdownLink(from, 'chat-ui.md')).toBe('docs/src/design/shell/chat-ui.md')
    expect(resolveMarkdownLink(from, './chat-ui.md')).toBe('docs/src/design/shell/chat-ui.md')
    expect(resolveMarkdownLink(from, '../intelligence/semantic-search.md')).toBe(
      'docs/src/design/intelligence/semantic-search.md'
    )
    expect(resolveMarkdownLink(from, '../../roadmap.md#m10')).toBe('docs/src/roadmap.md')
  })

  it('leaves absolute, fragment, and non-markdown links alone', () => {
    expect(resolveMarkdownLink(from, 'https://example.com/a.md')).toBeNull()
    expect(resolveMarkdownLink(from, '#section')).toBeNull()
    expect(resolveMarkdownLink(from, 'image.png')).toBeNull()
    expect(resolveMarkdownLink(from, '../../../../../../etc/passwd.md')).toBeNull()
  })
})
