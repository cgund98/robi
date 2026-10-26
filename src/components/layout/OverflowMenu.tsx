import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { For, onCleanup, onMount, Show } from 'solid-js'

import type { McpServer } from '../../api/mcp'
import { EllipsisHorizontal } from '../ui/icons'
import { apiHealthReason, watchApiHealth } from './apiHealth'
import styles from './OverflowMenu.module.css'
import { useMcpServers } from './useMcpServers'

/**
 * The secondary chrome, folded into one menu: one row per MCP server and a row
 * for a failing API health check. The button is absent while there is nothing
 * to show, so a healthy app with no MCP servers keeps the bar to three trailing
 * controls.
 */
export function OverflowMenu() {
  const servers = useMcpServers()
  onMount(() => {
    onCleanup(watchApiHealth())
  })

  const hasContent = () => servers().length > 0 || apiHealthReason() !== null

  return (
    <Show when={hasContent()}>
      <DropdownMenu>
        <DropdownMenu.Trigger class={styles.trigger} aria-label="More" title="More">
          <EllipsisHorizontal size={16} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content class={styles.panel}>
            <Show when={servers().length > 0}>
              <div class={styles.group}>
                <span class={styles.label}>MCP servers</span>
                <For each={servers()}>{(server) => <ServerRow server={server} />}</For>
              </div>
            </Show>
            <Show when={apiHealthReason()}>
              {(reason) => (
                <p class={styles.health} role="status">
                  {reason()}
                </p>
              )}
            </Show>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu>
    </Show>
  )
}

/**
 * One server: its icon, then `name · status · N tools` in one line, the same
 * facts the MCP settings tab lists. It is a menu item, so arrow keys reach it
 * and choosing it closes the menu.
 */
function ServerRow(props: { server: McpServer }) {
  const name = () => props.server.title || props.server.id
  const status = () => serverStatusLabel(props.server.status)
  const detail = () =>
    props.server.status === 'connected' ? ` · ${props.server.tool_count} tools` : ''

  return (
    <DropdownMenu.Item
      class={styles.server}
      as="a"
      href="/settings/mcp"
      title={`${name()} · ${status()}${detail()}`}
      aria-label={`${name()}, ${status()}${detail()}`}
    >
      <span class={`${styles.serverMark} ${serverMarkClass(props.server.status)}`} aria-hidden>
        <Show
          when={props.server.icon}
          fallback={<span>{props.server.id.slice(0, 1).toUpperCase()}</span>}
        >
          <img src={props.server.icon ?? ''} alt="" />
        </Show>
      </span>
      <span class={styles.serverText}>
        <span class={styles.serverName}>{name()}</span>
        <span class={styles.dot}> · </span>
        <span class={statusClass(props.server.status)}>{status()}</span>
        <Show when={detail()}>
          <span class={styles.dot}>{detail()}</span>
        </Show>
      </span>
    </DropdownMenu.Item>
  )
}

function serverStatusLabel(status: string): string {
  if (status === 'connected' || status === 'starting' || status === 'failed') {
    return status
  }
  return 'disconnected'
}

function statusClass(status: string): string {
  if (status === 'connected') {
    return styles.statusConnected
  }
  if (status === 'failed') {
    return styles.statusFailed
  }
  return styles.statusMuted
}

function serverMarkClass(status: string): string {
  if (status === 'connected') {
    return styles.serverConnected
  }
  if (status === 'failed') {
    return styles.serverFailed
  }
  if (status === 'starting') {
    return styles.serverStarting
  }
  return styles.serverDisconnected
}
