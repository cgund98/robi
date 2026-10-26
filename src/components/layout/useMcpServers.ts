import { createEffect, on } from 'solid-js'

import type { McpServer } from '../../api/mcp'
import { mcp } from '../../state/mcpStore'
import { workspaces } from '../../state/workspaceStore'

/**
 * The open workspace's MCP servers, loaded when the workspace changes.
 *
 * The chrome reads this to decide whether the overflow menu has anything to
 * show, so the fetch cannot live inside the tray that the menu renders: that
 * tray mounts only when the menu opens.
 */
export function useMcpServers(): () => McpServer[] {
  createEffect(
    on(
      () => workspaces.activeWorkspaceId,
      (id) => {
        if (!id) {
          return
        }
        void mcp.refresh(id)
      }
    )
  )
  return () => (mcp.workspaceId === workspaces.activeWorkspaceId ? mcp.servers : [])
}
