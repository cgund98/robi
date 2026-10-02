import { isTauri } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

/**
 * A directory path to open as a workspace, or null when the user cancels.
 *
 * The desktop window uses the system folder dialog. A normal browser cannot
 * return an absolute path, so it asks for one.
 */
export async function pickWorkspaceRoot(): Promise<string | null> {
  if (isTauri()) {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Add workspace'
    })
    return selected
  }

  const root = window.prompt('Absolute path of the workspace root')
  if (root == null) {
    return null
  }
  const trimmed = root.trim()
  return trimmed.length === 0 ? null : trimmed
}
