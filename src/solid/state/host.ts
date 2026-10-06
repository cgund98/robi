import { createRoot } from 'solid-js'
import { createStore, reconcile, type SetStoreFunction } from 'solid-js/store'

import { createChatState, type ChatState } from '../../state/chatStore'
import { createErrorLog, type ErrorLogState } from '../../state/errorLog'
import { createIndexState, type IndexStore } from '../../state/indexStore'
import { createMcpState, type McpState } from '../../state/mcpStore'
import { createRequestLog, type RequestLogState } from '../../state/requestLog'
import type { StoreGet, StoreSet } from '../../state/storeDeps'
import { createUiScaleState, type UiScaleState } from '../../state/uiScaleStore'
import { createWorkspaceState, type WorkspaceState } from '../../state/workspaceStore'

export let chat: ChatState = undefined!
export let workspaces: WorkspaceState = undefined!
export let mcp: McpState = undefined!
export let index: IndexStore = undefined!
export let uiScale: UiScaleState = undefined!
export let errors: ErrorLogState = undefined!
export let requests: RequestLogState = undefined!

let patchChatState: StoreSet<ChatState> = () => {}
let patchWorkspaceState: StoreSet<WorkspaceState> = () => {}

/** Shallow merge. Each top-level key is replaced, the same as Zustand's `set`. */
function hostStore<T extends object>(
  setup: (set: StoreSet<T>, get: StoreGet<T>) => T
): {
  state: T
  set: StoreSet<T>
} {
  let state!: T
  let write!: SetStoreFunction<T>
  const get = () => state
  const set: StoreSet<T> = (partial) => {
    const next = typeof partial === 'function' ? partial(state) : partial
    const apply = write as (key: keyof T, value: unknown) => void
    for (const key of Object.keys(next) as (keyof T)[]) {
      apply(key, reconcile(next[key], { merge: false }))
    }
  }
  const initial = setup(set, get)
  const [proxy, setState] = createStore(initial)
  state = proxy
  write = setState
  return { state: proxy, set }
}

let ready = false

/** One Solid store per shared setup. Call once, before the first render. */
export function installSolidStores(): void {
  if (ready) {
    return
  }
  ready = true
  createRoot(() => {
    const errorHost = hostStore<ErrorLogState>((set) => createErrorLog(set))
    errors = errorHost.state
    const requestHost = hostStore<RequestLogState>((set) => createRequestLog(set))
    requests = requestHost.state
    const workspaceHost = hostStore<WorkspaceState>((set, get) =>
      createWorkspaceState(set, get, {
        reportError: (message, sessionId) => errors.report(message, sessionId)
      })
    )
    workspaces = workspaceHost.state
    patchWorkspaceState = workspaceHost.set
    const workspaceId = () => workspaces.activeWorkspaceId
    const mcpHost = hostStore<McpState>((set) =>
      createMcpState(set, { activeWorkspaceId: workspaceId })
    )
    mcp = mcpHost.state
    const indexHost = hostStore<IndexStore>((set, get) =>
      createIndexState(set, get, { activeWorkspaceId: workspaceId })
    )
    index = indexHost.state
    const scaleHost = hostStore<UiScaleState>((set, get) => createUiScaleState(set, get))
    uiScale = scaleHost.state
    const chatHost = hostStore<ChatState>((set, get) =>
      createChatState(set, get, {
        reportError: (message, sessionId) => errors.report(message, sessionId),
        activeWorkspaceId: workspaceId
      })
    )
    chat = chatHost.state
    patchChatState = chatHost.set
  })
}

export function patchChat(partial: Partial<ChatState>): void {
  patchChatState(partial)
}

export function patchWorkspaces(partial: Partial<WorkspaceState>): void {
  patchWorkspaceState(partial)
}
