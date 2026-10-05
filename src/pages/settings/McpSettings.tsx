import { useCallback, useEffect, useState } from 'react'

import { getMcpConfig, type McpConfig, type McpServer } from '../../api/mcp'
import { useMcpStore } from '../../state/mcpStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './Settings.module.css'

const EMPTY_CONFIG = `{
  "mcpServers": {}
}`

export function McpSettings() {
  const workspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const loadedId = useMcpStore((state) => state.workspaceId)
  const loadedServers = useMcpStore((state) => state.servers)
  const servers = loadedId === workspaceId ? loadedServers : []

  const [config, setConfig] = useState<McpConfig | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const [requestedWorkspace, setRequestedWorkspace] = useState(workspaceId)
  if (requestedWorkspace !== workspaceId) {
    setRequestedWorkspace(workspaceId)
    setConfig(null)
    setError(null)
    setLoading(Boolean(workspaceId))
  }

  const refresh = useCallback(async () => {
    if (!workspaceId) {
      return
    }
    setLoading(true)
    setError(null)
    try {
      const [next] = await Promise.all([
        getMcpConfig(workspaceId),
        useMcpStore.getState().refresh(workspaceId)
      ])
      setConfig(next)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to load MCP config')
    } finally {
      setLoading(false)
    }
  }, [workspaceId])

  useEffect(() => {
    if (!workspaceId) {
      return
    }
    let cancelled = false
    // The server list is the same one the top-bar tray reads.
    void useMcpStore.getState().refresh(workspaceId)
    void getMcpConfig(workspaceId)
      .then((next) => {
        if (!cancelled) {
          setConfig(next)
          setError(null)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load MCP config')
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [workspaceId])

  const logsRoot = mcpLogsRoot(config?.user_path)

  return (
    <>
      <div className={styles.titleRow}>
        <h1 className={styles.title}>MCP</h1>
        <button
          type="button"
          className={styles.refresh}
          onClick={() => {
            void refresh()
          }}
          disabled={!workspaceId || loading}
        >
          {loading ? 'Refreshing' : 'Refresh'}
        </button>
      </div>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>Servers</h2>
        <p className={styles.sectionHint}>
          The servers this workspace is configured to use, and the state Robi has for each. The same
          list draws the marks in the top bar.
        </p>
        <div className={styles.card}>
          {servers.length === 0 ? (
            <div className={styles.row}>
              <div className={styles.copy}>
                <div className={styles.label}>No servers yet</div>
                <div className={styles.hint}>Add one in a config file below.</div>
              </div>
            </div>
          ) : (
            servers.map((server) => <ServerRow key={server.id} server={server} />)
          )}
        </div>
      </section>

      {config ? (
        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>Config files</h2>
          <ConfigFile label="Your servers" path={config.user_path} text={config.user_text} />
          <ConfigFile
            label="Project servers"
            path={config.project_path}
            text={config.project_text}
          />
        </section>
      ) : null}

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>Troubleshooting</h2>
        <p className={styles.sectionHint}>
          Each server writes its own log: the MCP messages in both directions and the server's own
          stderr. When a server fails to start, or a tool returns something unexpected, open the
          file for that server. Header and env values are never written, and old files are pruned
          after seven days.
        </p>
        <div className={styles.card}>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>Log location</div>
              <code className={styles.path}>{logsRoot}</code>
              <div className={styles.hint}>One directory per server, named by its id.</div>
            </div>
          </div>
          {servers.length > 0 ? (
            <div className={styles.row}>
              <div className={styles.copy}>
                <div className={styles.label}>This workspace's servers</div>
                <ul className={styles.logPaths}>
                  {servers.map((server) => (
                    <li key={server.id}>
                      <code className={styles.path}>{serverLogDir(logsRoot, server.id)}</code>
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          ) : null}
        </div>
      </section>

      {error ? <p className={`${styles.status} ${styles.statusError}`}>{error}</p> : null}
    </>
  )
}

function ServerRow({ server }: { server: McpServer }) {
  return (
    <div className={styles.row}>
      <div className={styles.serverIdentity}>
        <span className={`${styles.serverMark} ${serverMarkClass(server.status)}`} aria-hidden>
          {server.icon ? (
            <img src={server.icon} alt="" />
          ) : (
            <span>{server.id.slice(0, 1).toUpperCase()}</span>
          )}
        </span>
        <div className={styles.copy}>
          <div className={styles.label}>{server.title || server.id}</div>
          <div className={styles.hint}>
            <span>{server.id}</span>
            {' · '}
            <span className={serverStatusClass(server.status)}>
              {serverStatusLabel(server.status)}
            </span>
            {server.status === 'connected' ? ` · ${server.tool_count} tools` : ''}
          </div>
        </div>
      </div>
    </div>
  )
}

function ConfigFile({ label, path, text }: { label: string; path: string; text?: string | null }) {
  return (
    <section className={styles.card}>
      <div className={`${styles.copy} ${styles.configHead}`}>
        <div className={styles.label}>{label}</div>
        <code className={styles.path}>{path}</code>
      </div>
      <pre className={styles.config}>{text || EMPTY_CONFIG}</pre>
    </section>
  )
}

/** `~/.robi/logs/mcp`, from the absolute `<home>/.robi/mcp.json` path when we have it. */
function mcpLogsRoot(userPath?: string | null): string {
  if (!userPath) {
    return '~/.robi/logs/mcp'
  }
  const cut = Math.max(userPath.lastIndexOf('/'), userPath.lastIndexOf('\\'))
  const home = cut > 0 ? userPath.slice(0, cut) : userPath
  const sep = userPath.includes('\\') && !userPath.includes('/') ? '\\' : '/'
  return `${home}${sep}logs${sep}mcp`
}

function serverLogDir(root: string, serverId: string): string {
  const sep = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  return `${root}${sep}${serverId}${sep}`
}

function serverStatusLabel(status: string): string {
  if (status === 'connected' || status === 'starting' || status === 'failed') {
    return status
  }
  return 'disconnected'
}

function serverStatusClass(status: string): string {
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
