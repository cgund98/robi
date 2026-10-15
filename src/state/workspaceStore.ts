import { mountStore } from './storeDeps'

import { focusMcp } from '../api/mcp'
import { createWorkspace, deleteWorkspace, listWorkspaces, type Workspace } from '../api/workspaces'
import type { ErrorReporter, StoreGet, StoreSet } from './storeDeps'
import { errors } from './errorLog'

function noteError(deps: ErrorReporter, message: string): string {
  deps.reportError(message, null)
  return message
}

const ACTIVE_WORKSPACE_KEY = 'robi.activeWorkspaceId'
const RECENT_WORKSPACES_KEY = 'robi.recentWorkspaceIds'

export type WorkspaceState = {
  workspaces: Workspace[]
  /** Workspace ids, most recently selected first. The navbar menu uses this order. */
  recentWorkspaceIds: string[]
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

function readRecentIds(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_WORKSPACES_KEY)
    if (!raw) {
      return []
    }
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed) || !parsed.every((id) => typeof id === 'string')) {
      return []
    }
    return parsed
  } catch {
    return []
  }
}

function writeRecentIds(ids: string[]) {
  try {
    localStorage.setItem(RECENT_WORKSPACES_KEY, JSON.stringify(ids))
  } catch {
    // The menu still works for this page load when storage is blocked.
  }
}

/** Most recently selected first. Ids with no recorded use keep their incoming order. */
export function orderByRecentUse(list: Workspace[], recentIds: readonly string[]): Workspace[] {
  const rank = new Map(recentIds.map((id, index) => [id, index]))
  return list
    .map((workspace, index) => ({ workspace, index }))
    .sort((left, right) => {
      const leftRank = rank.get(left.workspace.id)
      const rightRank = rank.get(right.workspace.id)
      if (leftRank !== undefined && rightRank !== undefined && leftRank !== rightRank) {
        return leftRank - rightRank
      }
      if (leftRank !== undefined) {
        return -1
      }
      if (rightRank !== undefined) {
        return 1
      }
      return left.index - right.index
    })
    .map((entry) => entry.workspace)
}

function rememberUse(recentIds: readonly string[], id: string): string[] {
  return [id, ...recentIds.filter((item) => item !== id)]
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
    recentWorkspaceIds: readRecentIds(),
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
        const recentWorkspaceIds = activeWorkspaceId
          ? rememberUse(get().recentWorkspaceIds, activeWorkspaceId)
          : get().recentWorkspaceIds
        writeRecentIds(recentWorkspaceIds)
        set({ workspaces, recentWorkspaceIds, activeWorkspaceId, loaded: true, error: null })
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
      const recentWorkspaceIds = rememberUse(get().recentWorkspaceIds, id)
      writeRecentIds(recentWorkspaceIds)
      set({ activeWorkspaceId: id, recentWorkspaceIds, error: null })
      focusOpenWorkspace(id)
    },

    addWorkspace: async (root) => {
      set({ error: null })
      try {
        const workspace = await createWorkspace(root)
        const rest = get().workspaces.filter((item) => item.id !== workspace.id)
        writeActiveId(workspace.id)
        const recentWorkspaceIds = rememberUse(get().recentWorkspaceIds, workspace.id)
        writeRecentIds(recentWorkspaceIds)
        set({
          workspaces: [workspace, ...rest],
          recentWorkspaceIds,
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

const workspaceHost = mountStore<WorkspaceState>((set, get) =>
  createWorkspaceState(set, get, {
    reportError: (message) => errors.report(message, null)
  })
)

export const workspaces = workspaceHost.state

export function patchWorkspaces(partial: Partial<WorkspaceState>): void {
  workspaceHost.set(partial)
}
