import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'

import { listMcpServers, type McpServer } from '../../api/mcp'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './McpTray.module.css'

export function McpTray() {
  const workspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const [loaded, setLoaded] = useState<{ id: string; servers: McpServer[] } | null>(null)
  const servers = loaded?.id === workspaceId ? loaded.servers : []

  useEffect(() => {
    if (!workspaceId) {
      return
    }
    const id = workspaceId
    let cancelled = false
    const load = () => {
      void listMcpServers(id)
        .then((rows) => {
          if (!cancelled) {
            setLoaded({ id, servers: rows })
          }
        })
        .catch(() => {
          if (!cancelled) {
            setLoaded({ id, servers: [] })
          }
        })
    }
    load()
    const timer = window.setInterval(load, 4000)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [workspaceId])

  if (servers.length === 0) {
    return null
  }

  return (
    <div className={styles.tray} aria-label="MCP servers">
      {servers.map((server) => (
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
