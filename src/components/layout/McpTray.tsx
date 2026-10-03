import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'

import { listMcpServers, type McpServer } from '../../api/mcp'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './Sidebar.module.css'

export function McpTray() {
  const workspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const [servers, setServers] = useState<McpServer[]>([])

  useEffect(() => {
    if (!workspaceId) {
      setServers([])
      return
    }
    let cancelled = false
    const load = () => {
      void listMcpServers(workspaceId)
        .then((rows) => {
          if (!cancelled) {
            setServers(rows)
          }
        })
        .catch(() => {
          if (!cancelled) {
            setServers([])
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
    <section className={styles.mcpSection} aria-label="MCP servers">
      <Link className={styles.mcpHeader} to="/settings/mcp">
        MCP
      </Link>
      <div className={styles.mcpTray}>
        {servers.map((server) => (
          <span
            key={server.id}
            className={`${styles.mcpMark} ${markClass(server.status)}`}
            title={markTitle(server)}
          >
            {server.icon ? (
              <img src={server.icon} alt="" />
            ) : (
              <span aria-hidden>{server.id.slice(0, 1).toUpperCase()}</span>
            )}
          </span>
        ))}
      </div>
    </section>
  )
}

function markClass(status: string): string {
  if (status === 'connected') {
    return styles.mcpConnected
  }
  if (status === 'failed') {
    return styles.mcpFailed
  }
  if (status === 'starting') {
    return styles.mcpStarting
  }
  return styles.mcpDisconnected
}

function markTitle(server: McpServer): string {
  const name = server.title || server.id
  const tools = server.status === 'connected' ? `, ${server.tool_count} tools` : ''
  return `${name} · ${server.status}${tools}`
}
