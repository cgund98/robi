import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { afterEach, describe, expect, it } from 'vitest'

import { ViewToggle } from './ViewToggle'

function LocationProbe() {
  const { pathname } = useLocation()
  return <span data-testid="pathname">{pathname}</span>
}

function renderToggle(docsOpen: boolean, initialEntries: string[]) {
  render(
    <MemoryRouter initialEntries={initialEntries}>
      <ViewToggle docsOpen={docsOpen} />
      <LocationProbe />
    </MemoryRouter>
  )
}

describe('ViewToggle', () => {
  afterEach(() => {
    cleanup()
  })

  it('opens documentation from the docs icon', () => {
    renderToggle(false, ['/'])
    const docs = screen.getByRole('button', { name: 'Documentation' })
    expect(docs.getAttribute('aria-pressed')).toBe('false')
    expect(docs.getAttribute('title')).toBe('Documentation')

    fireEvent.click(docs)
    expect(screen.getByTestId('pathname').textContent).toBe('/docs')
  })

  it('returns to chat from the chat icon', () => {
    renderToggle(true, ['/docs'])
    const chat = screen.getByRole('button', { name: 'Chat' })
    expect(chat.getAttribute('aria-pressed')).toBe('false')
    expect(chat.getAttribute('title')).toBe('Chat')

    fireEvent.click(chat)
    expect(screen.getByTestId('pathname').textContent).toBe('/')
  })
})
