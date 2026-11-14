import { MemoryRouter, Route, createMemoryHistory } from '@solidjs/router'
import { render } from 'solid-js/web'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TopBar } from './TopBar'
import { patchWorkspaces } from '../../state/workspaceStore'

beforeEach(() => {
  // The bar's overflow polls the API's health. Answer it so the test does no
  // network work and no mark appears.
  vi.stubGlobal(
    'fetch',
    vi.fn(() => Promise.resolve(new Response('Ok', { status: 200 })))
  )
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function mount() {
  const history = createMemoryHistory()
  history.set({ value: '/sessions/s1', replace: true, scroll: false })
  const container = document.createElement('div')
  document.body.appendChild(container)
  const dispose = render(
    () => (
      <MemoryRouter history={history} root={(props) => props.children}>
        <Route
          path="/*"
          component={() => (
            <TopBar
              sessionTitle="Fix the parser"
              sessions={[]}
              activeSessionId="s1"
              draftSelected={false}
              phaseBySession={{}}
              onSelectSession={() => {}}
              onNewSession={() => {}}
              onRenameSession={() => {}}
              onDeleteSession={() => {}}
            />
          )}
        />
      </MemoryRouter>
    ),
    container
  )
  return { container, dispose }
}

describe('TopBar', () => {
  it('names every control', () => {
    const { container, dispose } = mount()
    const labels = [...container.querySelectorAll('header button')].map((button) =>
      button.getAttribute('aria-label')
    )
    expect(labels).toContain('New chat')
    expect(labels).toContain('Settings')
    expect(labels).toContain('Back')
    expect(labels).toContain('Forward')
    expect(labels?.some((label) => label?.startsWith('Workspace'))).toBe(true)
    expect(labels).toContain('Fix the parser — recents')
    dispose()
  })

  it('names the active workspace in the bar, not only in the menu', () => {
    patchWorkspaces({
      workspaces: [{ id: 'w1', name: 'Robi', root: '/Users/me/robi', created_at: 'now' }],
      activeWorkspaceId: 'w1'
    })
    const { container, dispose } = mount()
    const trigger = container.querySelector('button[aria-label="Workspace: Robi"]')
    expect(trigger?.textContent).toContain('Robi')
    expect(trigger?.querySelector('svg')).not.toBeNull()
    dispose()
    patchWorkspaces({ workspaces: [], activeWorkspaceId: null })
  })

  it('carries the session title and no review entry point', () => {
    const { container, dispose } = mount()
    expect(container.textContent).toContain('Fix the parser')
    expect(container.textContent).not.toContain('Review')
    dispose()
  })
})
