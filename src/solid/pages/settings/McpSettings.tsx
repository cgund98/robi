/** @jsxImportSource solid-js */
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'

import { getMcpConfig, type McpConfig, type McpServer } from '../../../api/mcp'
import styles from '../../../pages/settings/Settings.module.css'
import { mcp, workspaces } from '../../state/host'

const EMPTY_CONFIG = `{
  "mcpServers": {}
}`

export function McpSettings() {
  const servers = () => (mcp.workspaceId === workspaces.activeWorkspaceId ? mcp.servers : [])
  const [config, setConfig] = createSignal<McpConfig | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)

  createEffect(() => {
    const id = workspaces.activeWorkspaceId
    setConfig(null)
    setError(null)
    if (!id) {
      setLoading(false)
      return
    }
    setLoading(true)
    let cancelled = false
    void mcp.refresh(id)
    void getMcpConfig(id)
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
    onCleanup(() => {
      cancelled = true
    })
  })

  async function refresh() {
    const id = workspaces.activeWorkspaceId
    if (!id) {
      return
    }
    setLoading(true)
    setError(null)
    try {
      const [next] = await Promise.all([getMcpConfig(id), mcp.refresh(id)])
      setConfig(next)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to load MCP config')
    } finally {
      setLoading(false)
    }
  }

  const logsRoot = () => mcpLogsRoot(config()?.user_path)

  return (
    <>
      <div class={styles.titleRow}>
        <h1 class={styles.title}>MCP</h1>
        <button
          type="button"
          class={styles.refresh}
          onClick={() => void refresh()}
          disabled={!workspaces.activeWorkspaceId || loading()}
        >
          {loading() ? 'Refreshing' : 'Refresh'}
        </button>
      </div>

      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Servers</h2>
        <p class={styles.sectionHint}>
          The servers this workspace is configured to use, and the state Robi has for each. The same
          list draws the marks in the top bar.
        </p>
        <div class={styles.card}>
          <Show
            when={servers().length > 0}
            fallback={
              <div class={styles.row}>
                <div class={styles.copy}>
                  <div class={styles.label}>No servers yet</div>
                  <div class={styles.hint}>Add one in a config file below.</div>
                </div>
              </div>
            }
          >
            <For each={servers()}>{(server) => <ServerRow server={server} />}</For>
          </Show>
        </div>
      </section>

      <Show when={config()}>
        {(current) => (
          <section class={styles.section}>
            <h2 class={styles.sectionTitle}>Config files</h2>
            <ConfigFile
              label="Your servers"
              path={current().user_path}
              text={current().user_text}
            />
            <ConfigFile
              label="Project servers"
              path={current().project_path}
              text={current().project_text}
            />
          </section>
        )}
      </Show>

      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Troubleshooting</h2>
        <p class={styles.sectionHint}>
          Each server writes its own log: the MCP messages in both directions and the server's own
          stderr. When a server fails to start, or a tool returns something unexpected, open the
          file for that server. Header and env values are never written, and old files are pruned
          after seven days.
        </p>
        <div class={styles.card}>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Log location</div>
              <code class={styles.path}>{logsRoot()}</code>
              <div class={styles.hint}>One directory per server, named by its id.</div>
            </div>
          </div>
          <Show when={servers().length > 0}>
            <div class={styles.row}>
              <div class={styles.copy}>
                <div class={styles.label}>This workspace's servers</div>
                <ul class={styles.logPaths}>
                  <For each={servers()}>
                    {(server) => (
                      <li>
                        <code class={styles.path}>{serverLogDir(logsRoot(), server.id)}</code>
                      </li>
                    )}
                  </For>
                </ul>
              </div>
            </div>
          </Show>
        </div>
      </section>

      <Show when={error()}>
        <p class={`${styles.status} ${styles.statusError}`}>{error()}</p>
      </Show>
    </>
  )
}

function ServerRow(props: { server: McpServer }) {
  return (
    <div class={styles.row}>
      <div class={styles.serverIdentity}>
        <span class={`${styles.serverMark} ${serverMarkClass(props.server.status)}`} aria-hidden>
          <Show
            when={props.server.icon}
            fallback={<span>{props.server.id.slice(0, 1).toUpperCase()}</span>}
          >
            <img src={props.server.icon ?? ''} alt="" />
          </Show>
        </span>
        <div class={styles.copy}>
          <div class={styles.label}>{props.server.title || props.server.id}</div>
          <div class={styles.hint}>
            <span>{props.server.id}</span>
            {' · '}
            <span class={serverStatusClass(props.server.status)}>
              {serverStatusLabel(props.server.status)}
            </span>
            {props.server.status === 'connected' ? ` · ${props.server.tool_count} tools` : ''}
          </div>
        </div>
      </div>
    </div>
  )
}

function ConfigFile(props: { label: string; path: string; text?: string | null }) {
  return (
    <section class={styles.card}>
      <div class={`${styles.copy} ${styles.configHead}`}>
        <div class={styles.label}>{props.label}</div>
        <code class={styles.path}>{props.path}</code>
      </div>
      <pre class={styles.config}>{props.text || EMPTY_CONFIG}</pre>
    </section>
  )
}

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
