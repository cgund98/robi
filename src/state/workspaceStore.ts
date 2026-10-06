import { create } from 'zustand'

import { focusMcp } from '../api/mcp'
import { createWorkspace, deleteWorkspace, listWorkspaces, type Workspace } from '../api/workspaces'
import type { ErrorReporter, StoreGet, StoreSet } from './storeDeps'
import { useErrorLog } from './errorLog'

function noteError(deps: ErrorReporter, message: string): string {
  deps.reportError(message, null)
  return message
}

const ACTIVE_WORKSPACE_KEY = 'robi.activeWorkspaceId'

export type WorkspaceState = {
  workspaces: Workspace[]
  activeWorkspaceId: string | null
  loaded: boolean
  error: string | null
  loadWorkspaces: () => Promise<void>
  selectWorkspace: (id: string) => void
  addWorkspace: (root: string) => Promise<void>
  removeWorkspace: (id: string) => Promise<void>
}

function readActiveId(): string | null {
  try {
    return localStorage.getItem(ACTIVE_WORKSPACE_KEY)
  } catch {
    return null
  }
}

function writeActiveId(id: string | null) {
  try {
    if (id === null) {
      localStorage.removeItem(ACTIVE_WORKSPACE_KEY)
      return
    }
    localStorage.setItem(ACTIVE_WORKSPACE_KEY, id)
  } catch {
    // The menu still works for this page load when storage is blocked.
  }
}

function errorText(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback
}

function focusOpenWorkspace(id: string | null) {
  if (!id) {
    return
  }
  void focusMcp(id).catch(() => {
    // Status stays disconnected until the next switch or agent turn.
  })
}

function chooseActive(workspaces: Workspace[], preferred: string | null): string | null {
  if (preferred && workspaces.some((workspace) => workspace.id === preferred)) {
    return preferred
  }
  return workspaces[0]?.id ?? null
}

export function createWorkspaceState(
  set: StoreSet<WorkspaceState>,
  get: StoreGet<WorkspaceState>,
  deps: ErrorReporter
): WorkspaceState {
  return {
    workspaces: [],
    activeWorkspaceId: readActiveId(),
    loaded: false,
    error: null,

    loadWorkspaces: async () => {
      try {
        const workspaces = await listWorkspaces()
        const activeWorkspaceId = chooseActive(
          workspaces,
          readActiveId() ?? get().activeWorkspaceId
        )
        writeActiveId(activeWorkspaceId)
        set({ workspaces, activeWorkspaceId, loaded: true, error: null })
        focusOpenWorkspace(activeWorkspaceId)
      } catch (err) {
        set({
          loaded: true,
          error: noteError(deps, errorText(err, 'Failed to load workspaces'))
        })
      }
    },

    selectWorkspace: (id) => {
      if (!get().workspaces.some((workspace) => workspace.id === id)) {
        return
      }
      if (get().activeWorkspaceId === id) {
        return
      }
      writeActiveId(id)
      set({ activeWorkspaceId: id, error: null })
      focusOpenWorkspace(id)
    },

    addWorkspace: async (root) => {
      set({ error: null })
      try {
        const workspace = await createWorkspace(root)
        const rest = get().workspaces.filter((item) => item.id !== workspace.id)
        writeActiveId(workspace.id)
        set({
          workspaces: [workspace, ...rest],
          activeWorkspaceId: workspace.id,
          loaded: true,
          error: null
        })
        focusOpenWorkspace(workspace.id)
      } catch (err) {
        set({ error: noteError(deps, errorText(err, 'Failed to add workspace')) })
      }
    },

    removeWorkspace: async (id) => {
      set({ error: null })
      try {
        await deleteWorkspace(id)
      } catch (err) {
        set({ error: noteError(deps, errorText(err, 'Failed to remove workspace')) })
        return
      }
      const workspaces = get().workspaces.filter((workspace) => workspace.id !== id)
      const activeWorkspaceId =
        get().activeWorkspaceId === id ? (workspaces[0]?.id ?? null) : get().activeWorkspaceId
      writeActiveId(activeWorkspaceId)
      set({ workspaces, activeWorkspaceId, error: null })
      if (activeWorkspaceId !== id) {
        focusOpenWorkspace(activeWorkspaceId)
      }
    }
  }
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) =>
  createWorkspaceState(set, get, {
    reportError: (message) => useErrorLog.getState().report(message, null)
  })
)
