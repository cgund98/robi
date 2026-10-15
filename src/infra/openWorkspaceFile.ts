import { isTauri } from '@tauri-apps/api/core'
import { openPath } from '@tauri-apps/plugin-opener'

import { openWorkspaceFileCommand, resolveWorkspacePath } from '../api/workspaces'
import { getSetting, SETTING_KEYS } from '../api/settings'
import { ApiError } from '../api/sessions'
import { workspaces } from '../state/workspaceStore'

/** The configured opener failed. The message is the API error text. */
export class OpenCommandError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'OpenCommandError'
  }
}

/**
 * Open a workspace-relative file in the app the operating system uses for it,
 * or with `file_open_command` when that setting is set.
 *
 * The desktop window asks the API for the absolute path, then the opener.
 * A browser has no opener, so the call does nothing.
 * `line` is the 1-based line the command template receives.
 */
export async function openWorkspaceFile(relativePath: string, line?: number): Promise<void> {
  if (!isTauri()) {
    return
  }
  const workspaceId = workspaces.activeWorkspaceId
  if (!workspaceId) {
    return
  }
  const setting = await getSetting(SETTING_KEYS.fileOpenCommand)
  const command = setting.value?.trim() ?? ''
  if (command.length > 0) {
    try {
      await openWorkspaceFileCommand(workspaceId, relativePath, line)
    } catch (err: unknown) {
      if (err instanceof ApiError && err.status === 404) {
        const absolute = await resolveWorkspacePath(workspaceId, relativePath)
        await openPath(absolute)
        return
      }
      const message = err instanceof Error ? err.message : 'Failed to open file'
      throw new OpenCommandError(message)
    }
    return
  }
  const absolute = await resolveWorkspacePath(workspaceId, relativePath)
  await openPath(absolute)
}
