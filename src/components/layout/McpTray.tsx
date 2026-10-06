/** @jsxImportSource solid-js */
import { A } from '@solidjs/router'
import { createEffect, For, on, Show } from 'solid-js'

import type { McpServer } from '../../api/mcp'
import styles from './McpTray.module.css'
import { mcp } from '../../state/mcpStore'
import { workspaces } from '../../state/workspaceStore'

export function McpTray() {
  const visible = () => (mcp.workspaceId === workspaces.activeWorkspaceId ? mcp.servers : [])

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

  return (
    <Show when={visible().length > 0}>
      <div class={styles.tray} aria-label="MCP servers">
        <For each={visible()}>
          {(server) => (
            <A
              class={`${styles.mark} ${markClass(server.status)}`}
              href="/settings/mcp"
              title={markTitle(server)}
              aria-label={markTitle(server)}
            >
              {server.icon ? (
                <img src={server.icon} alt="" />
              ) : (
                <span aria-hidden="true">{server.id.slice(0, 1).toUpperCase()}</span>
              )}
            </A>
          )}
        </For>
      </div>
    </Show>
  )
}

function markClass(status: string): string {
  if (status === 'connected') {
    return styles.connected
  }
  if (status === 'failed') {
    return styles.failed
  }
  if (status === 'starting') {
    return styles.starting
  }
  return styles.disconnected
}

function markTitle(server: McpServer): string {
  const name = server.title || server.id
  const tools = server.status === 'connected' ? `, ${server.tool_count} tools` : ''
  return `${name} · ${server.status}${tools}`
}
