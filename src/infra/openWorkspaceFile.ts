import { isTauri } from '@tauri-apps/api/core'
import { openPath } from '@tauri-apps/plugin-opener'

import { resolveWorkspacePath } from '../api/workspaces'
import { workspaces } from '../state/workspaceStore'

/**
 * Open a workspace-relative file in the app the operating system uses for it.
 *
 * The desktop window asks the API for the absolute path, then the opener.
 * A browser has no opener, so the call does nothing.
 */
export async function openWorkspaceFile(relativePath: string): Promise<void> {
  if (!isTauri()) {
    return
  }
  const workspaceId = workspaces.activeWorkspaceId
  if (!workspaceId) {
    return
  }
  const absolute = await resolveWorkspacePath(workspaceId, relativePath)
  await openPath(absolute)
}
