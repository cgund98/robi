import { useEffect } from 'react'
import { Link } from 'react-router-dom'

import type { McpServer } from '../../api/mcp'
import { useMcpStore } from '../../state/mcpStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './McpTray.module.css'

export function McpTray() {
  const workspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const loadedId = useMcpStore((state) => state.workspaceId)
  const servers = useMcpStore((state) => state.servers)
  const visible = loadedId === workspaceId ? servers : []

  useEffect(() => {
    if (!workspaceId) {
      return
    }
    void useMcpStore.getState().refresh(workspaceId)
  }, [workspaceId])

  if (visible.length === 0) {
    return null
  }

  return (
    <div className={styles.tray} aria-label="MCP servers">
      {visible.map((server) => (
        <Link
          key={server.id}
          className={`${styles.mark} ${markClass(server.status)}`}
          to="/settings/mcp"
          title={markTitle(server)}
          aria-label={markTitle(server)}
        >
          {server.icon ? (
            <img src={server.icon} alt="" />
          ) : (
            <span aria-hidden>{server.id.slice(0, 1).toUpperCase()}</span>
          )}
        </Link>
      ))}
    </div>
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
