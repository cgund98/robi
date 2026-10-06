/** Partial update, including Zustand's functional form. */
export type StoreSet<T> = (partial: Partial<T> | ((state: T) => Partial<T>)) => void

export type StoreGet<T> = () => T

export type ErrorReporter = {
  reportError: (message: string, sessionId?: string | null) => void
}

export type ActiveWorkspace = {
  activeWorkspaceId: () => string | null
}
