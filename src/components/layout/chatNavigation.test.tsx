/** @jsxImportSource solid-js */
import {
  MemoryRouter,
  Route,
  createMemoryHistory,
  useIsRouting,
  useLocation,
  useNavigate
} from '@solidjs/router'
import { createEffect, createSignal } from 'solid-js'
import { render } from 'solid-js/web'
import { describe, expect, it } from 'vitest'

type ChatState = { draft: boolean; activeId: string | null }

/**
 * Mirrors the route -> store reconciler and the "New chat" handler in
 * `ChatChrome`. The handler navigates before it writes the store, so the
 * reconciler sees `isRouting()` and bails instead of reading the stale
 * `/sessions/:id` route and undoing the draft. Writing the store first makes
 * one click not land.
 */
function Harness(props: { state: () => ChatState; setState: (next: ChatState) => void }) {
  const navigate = useNavigate()
  const location = useLocation()
  const isRouting = useIsRouting()
  const chatSessionId = () => /^\/sessions\/([^/]+)$/.exec(location.pathname)?.[1] ?? null

  createEffect(() => {
    const id = chatSessionId()
    if (isRouting() || !id) {
      return
    }
    const current = props.state()
    if (current.draft || current.activeId !== id) {
      props.setState({ draft: false, activeId: id })
    }
  })

  return (
    <button
      id="new-chat"
      onClick={() => {
        if (location.pathname !== '/') {
          navigate('/')
        }
        props.setState({ draft: true, activeId: null })
      }}
    >
      New chat
    </button>
  )
}

async function settle() {
  // The router commits the location in a microtask; flush a few turns.
  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()
}

function mountSessionRoute() {
  const [state, setState] = createSignal<ChatState>({ draft: false, activeId: 's1' })
  const history = createMemoryHistory()
  history.set({ value: '/sessions/s1', replace: true, scroll: false })
  const container = document.createElement('div')
  document.body.appendChild(container)
  const dispose = render(
    () => (
      <MemoryRouter history={history} root={(props) => props.children}>
        <Route path="/*" component={() => <Harness state={state} setState={setState} />} />
      </MemoryRouter>
    ),
    container
  )
  return { state, container, dispose }
}

describe('chat navigation', () => {
  it('one New chat click selects the draft from a session route', async () => {
    const { state, container, dispose } = mountSessionRoute()
    const button = container.querySelector<HTMLButtonElement>('#new-chat')!
    button.click()
    await settle()
    expect(state()).toEqual({ draft: true, activeId: null })
    dispose()
  })
})
