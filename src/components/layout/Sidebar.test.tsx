import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { ChatSession } from '../../api/sessions'
import { Sidebar } from './Sidebar'

function session(id: string): ChatSession {
  return { id, title: id } as ChatSession
}

function renderSidebar(count: number, activeSessionId = 's1') {
  render(
    <MemoryRouter>
      <Sidebar
        sessions={Array.from({ length: count }, (_, index) => session(`s${index + 1}`))}
        activeSessionId={activeSessionId}
        onSelectSession={vi.fn()}
        onNewSession={vi.fn()}
        onRenameSession={vi.fn()}
        onDeleteSession={vi.fn()}
      />
    </MemoryRouter>
  )
}

function sessionButton(title: string) {
  return screen.queryByRole('button', { name: (name) => name === title })
}

describe('Sidebar recents', () => {
  afterEach(() => {
    cleanup()
  })

  it('shows five sessions and reveals ten more at a time', () => {
    renderSidebar(20)
    expect(sessionButton('s5')).toBeTruthy()
    expect(sessionButton('s6')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Show more' }))
    expect(sessionButton('s15')).toBeTruthy()
    expect(sessionButton('s16')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Show more' }))
    expect(sessionButton('s20')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull()
  })

  it('keeps the open session visible when it is older than the window', () => {
    renderSidebar(12, 's8')
    expect(sessionButton('s8')).toBeTruthy()
    expect(sessionButton('s9')).toBeNull()
  })
})
