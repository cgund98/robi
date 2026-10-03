import { useCallback, useEffect, useState } from 'react'

import { getMcpConfig, type McpConfig } from '../../api/mcp'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './Settings.module.css'

const EMPTY_CONFIG = `{
  "mcpServers": {}
}`

export function McpSettings() {
  const workspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const [config, setConfig] = useState<McpConfig | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    if (!workspaceId) {
      setConfig(null)
      setError(null)
      return
    }
    setLoading(true)
    setError(null)
    try {
      setConfig(await getMcpConfig(workspaceId))
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to load MCP config')
    } finally {
      setLoading(false)
    }
  }, [workspaceId])

  useEffect(() => {
    void refresh()
  }, [refresh])

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
      {config ? (
        <>
          <ConfigFile label="Your servers" path={config.user_path} text={config.user_text} />
          <ConfigFile
            label="Project servers"
            path={config.project_path}
            text={config.project_text}
          />
        </>
      ) : null}
      {error ? <p className={`${styles.status} ${styles.statusError}`}>{error}</p> : null}
    </>
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
