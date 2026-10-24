import { A } from '@solidjs/router'
import { For, Show } from 'solid-js'

import type { McpServer } from '../../api/mcp'
import styles from './McpTray.module.css'
import { useMcpServers } from './useMcpServers'

/** A mark per configured server, each linking to MCP settings. */
export function McpTray() {
  const servers = useMcpServers()

  return (
    <Show when={servers().length > 0}>
      <div class={styles.tray} aria-label="MCP servers">
        <For each={servers()}>
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
