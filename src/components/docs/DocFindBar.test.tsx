import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { createRef } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { DocFindBar } from './DocFindBar'

function renderBar(overrides: Partial<Parameters<typeof DocFindBar>[0]> = {}) {
  const handlers = {
    onQuery: vi.fn(),
    onCaseSensitive: vi.fn(),
    onNext: vi.fn(),
    onPrevious: vi.fn(),
    onClose: vi.fn()
  }
  const props = {
    query: 'alpha',
    count: 3,
    current: 1,
    caseSensitive: false,
    inputRef: createRef<HTMLInputElement>(),
    ...handlers,
    ...overrides
  }
  render(<DocFindBar {...props} />)
  return handlers
}

describe('DocFindBar', () => {
  afterEach(() => {
    cleanup()
  })

  it('shows the one-based position of the active match', () => {
    renderBar()
    expect(screen.getByRole('status').textContent).toBe('2 of 3')
  })

  it('says No results when there are no matches', () => {
    renderBar({ query: 'zzz', count: 0, current: -1 })
    expect(screen.getByRole('status').textContent).toBe('No results')
  })

  it('shows nothing for an empty query', () => {
    renderBar({ query: '', count: 0, current: -1 })
    expect(screen.getByRole('status').textContent).toBe('')
  })

  it('steps with the previous and next buttons', () => {
    const handlers = renderBar()
    fireEvent.click(screen.getByRole('button', { name: 'Next match' }))
    fireEvent.click(screen.getByRole('button', { name: 'Previous match' }))
    expect(handlers.onNext).toHaveBeenCalledTimes(1)
    expect(handlers.onPrevious).toHaveBeenCalledTimes(1)
  })

  it('disables the step buttons when there are no matches', () => {
    renderBar({ query: 'zzz', count: 0, current: -1 })
    const next = screen.getByRole('button', { name: 'Next match' }) as HTMLButtonElement
    const previous = screen.getByRole('button', { name: 'Previous match' }) as HTMLButtonElement
    expect(next.disabled).toBe(true)
    expect(previous.disabled).toBe(true)
  })

  it('toggles match case', () => {
    const handlers = renderBar()
    const toggle = screen.getByRole('button', { name: 'Match case' })
    expect(toggle.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(toggle)
    expect(handlers.onCaseSensitive).toHaveBeenCalledWith(true)
  })

  it('closes', () => {
    const handlers = renderBar()
    fireEvent.click(screen.getByRole('button', { name: 'Close find' }))
    expect(handlers.onClose).toHaveBeenCalledTimes(1)
  })

  it('steps forward on Enter and backward on Shift+Enter', () => {
    const handlers = renderBar()
    const field = screen.getByRole('textbox', { name: 'Find in document' })
    fireEvent.keyDown(field, { key: 'Enter' })
    fireEvent.keyDown(field, { key: 'Enter', shiftKey: true })
    expect(handlers.onNext).toHaveBeenCalledTimes(1)
    expect(handlers.onPrevious).toHaveBeenCalledTimes(1)
  })

  it('reports a new query as it is typed', () => {
    const handlers = renderBar()
    fireEvent.change(screen.getByRole('textbox', { name: 'Find in document' }), {
      target: { value: 'beta' }
    })
    expect(handlers.onQuery).toHaveBeenCalledWith('beta')
  })
})
