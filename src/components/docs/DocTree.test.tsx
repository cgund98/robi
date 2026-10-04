import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { buildFileTree } from '../review/tree'
import { DocTree } from './DocTree'

const tree = buildFileTree(['docs/a.md', 'docs/nested/b.md', 'README.md'])

describe('DocTree', () => {
  afterEach(() => {
    cleanup()
  })

  it('renders leaf files and directory buttons expanded by default', () => {
    render(
      <DocTree
        nodes={tree}
        selected={null}
        collapsed={new Set()}
        onSelect={() => {}}
        onToggle={() => {}}
      />
    )
    const docs = screen.getByRole('button', { name: /docs/ })
    expect(docs.getAttribute('aria-expanded')).toBe('true')
    expect(screen.getByRole('button', { name: 'a.md' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'b.md' })).toBeTruthy()
  })

  it('reports a toggle when a directory is clicked', () => {
    const toggled: string[] = []
    render(
      <DocTree
        nodes={tree}
        selected={null}
        collapsed={new Set()}
        onSelect={() => {}}
        onToggle={(path) => toggled.push(path)}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /docs/ }))
    expect(toggled).toEqual(['docs'])
  })

  it('hides children of a collapsed directory', () => {
    render(
      <DocTree
        nodes={tree}
        selected={null}
        collapsed={new Set(['docs'])}
        onSelect={() => {}}
        onToggle={() => {}}
      />
    )
    const docs = screen.getByRole('button', { name: /docs/ })
    expect(docs.getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByRole('button', { name: 'a.md' })).toBeNull()
    expect(screen.getByRole('button', { name: 'README.md' })).toBeTruthy()
  })
})
