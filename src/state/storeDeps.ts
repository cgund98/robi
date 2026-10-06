import { createStore, reconcile, type SetStoreFunction } from 'solid-js/store'

/** Replace the listed top-level keys. A function receives the current state. */
export type StoreSet<T> = (partial: Partial<T> | ((state: T) => Partial<T>)) => void

export type StoreGet<T> = () => T

/** One Solid store. `set` replaces each given top-level key. */
export function mountStore<T extends object>(
  setup: (set: StoreSet<T>, get: StoreGet<T>) => T
): { state: T; set: StoreSet<T> } {
  const held: { state?: T; write?: SetStoreFunction<T> } = {}
  const get = () => held.state as T
  const set: StoreSet<T> = (partial) => {
    const next = typeof partial === 'function' ? partial(get()) : partial
    const apply = held.write as (key: keyof T, value: unknown) => void
    for (const key of Object.keys(next) as (keyof T)[]) {
      apply(key, reconcile(next[key], { merge: false }))
    }
  }
  const initial = setup(set, get)
  const [proxy, setState] = createStore(initial)
  held.state = proxy
  held.write = setState
  return { state: proxy, set }
}

export type ErrorReporter = {
  reportError: (message: string, sessionId?: string | null) => void
}

export type ActiveWorkspace = {
  activeWorkspaceId: () => string | null
}
