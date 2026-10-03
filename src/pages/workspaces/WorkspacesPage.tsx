import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { pickWorkspaceRoot } from '../../infra/pickWorkspaceRoot'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './WorkspacesPage.module.css'

export function WorkspacesPage() {
  const navigate = useNavigate()
  const workspaces = useWorkspaceStore((state) => state.workspaces)
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const loaded = useWorkspaceStore((state) => state.loaded)
  const error = useWorkspaceStore((state) => state.error)
  const loadWorkspaces = useWorkspaceStore((state) => state.loadWorkspaces)
  const selectWorkspace = useWorkspaceStore((state) => state.selectWorkspace)
  const addWorkspace = useWorkspaceStore((state) => state.addWorkspace)
  const removeWorkspace = useWorkspaceStore((state) => state.removeWorkspace)
  const [query, setQuery] = useState('')
  const [creating, setCreating] = useState(false)

  useEffect(() => {
    if (!loaded) {
      void loadWorkspaces()
    }
  }, [loaded, loadWorkspaces])

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const matched = needle
      ? workspaces.filter(
          (workspace) =>
            workspace.name.toLowerCase().includes(needle) ||
            workspace.root.toLowerCase().includes(needle)
        )
      : workspaces
    return matched
  }, [workspaces, query])

  async function handleNew() {
    setCreating(true)
    try {
      const root = await pickWorkspaceRoot()
      if (root == null) {
        return
      }
      await addWorkspace(root)
      if (useWorkspaceStore.getState().error) {
        return
      }
      navigate('/')
    } catch (err) {
      useWorkspaceStore.setState({
        error: err instanceof Error ? err.message : 'Failed to open the folder dialog'
      })
    } finally {
      setCreating(false)
    }
  }

  function openWorkspace(id: string) {
    selectWorkspace(id)
    navigate('/')
  }

  function handleRemove(id: string, name: string) {
    if (!window.confirm(`Remove “${name}”? Its sessions will be deleted.`)) {
      return
    }
    void removeWorkspace(id)
  }

  const empty = loaded && workspaces.length === 0

  return (
    <div className={styles.page}>
      <header className={styles.top}>
        <div className={styles.heading}>
          {activeWorkspaceId ? (
            <button type="button" className={styles.back} onClick={() => navigate('/')}>
              ← Chat
            </button>
          ) : null}
          <h1 className={styles.title}>Workspaces</h1>
        </div>
        <div className={styles.tools}>
          <label className={styles.search}>
            <SearchIcon />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search"
              aria-label="Search workspaces"
            />
          </label>
          <button
            type="button"
            className={styles.primary}
            onClick={() => void handleNew()}
            disabled={creating}
          >
            New workspace
          </button>
        </div>
      </header>

      {error ? (
        <div className={styles.banner} role="alert">
          {error}
        </div>
      ) : null}

      {!loaded ? (
        <p className={styles.status}>Loading…</p>
      ) : empty ? (
        <div className={styles.empty}>
          <FolderMark />
          <h2 className={styles.emptyTitle}>Looking to start a workspace?</h2>
          <p className={styles.emptyCopy}>
            Open a directory. Sessions stay with that folder, and the agent works from its root.
          </p>
          <button
            type="button"
            className={styles.emptyAction}
            onClick={() => void handleNew()}
            disabled={creating}
          >
            New workspace
          </button>
        </div>
      ) : (
        <ul className={styles.list}>
          {visible.length === 0 ? (
            <li className={styles.noMatch}>No workspaces match “{query.trim()}”.</li>
          ) : (
            visible.map((workspace) => {
              const current = workspace.id === activeWorkspaceId
              return (
                <li key={workspace.id}>
                  <article className={styles.card}>
                    <div className={styles.cardHead}>
                      <button
                        type="button"
                        className={styles.cardName}
                        onClick={() => openWorkspace(workspace.id)}
                      >
                        {workspace.name}
                        {current ? <span className={styles.current}>Current</span> : null}
                      </button>
                      <div className={styles.actions}>
                        <button
                          type="button"
                          className={styles.open}
                          onClick={() => openWorkspace(workspace.id)}
                        >
                          Open
                        </button>
                        <button
                          type="button"
                          className={styles.remove}
                          aria-label={`Remove ${workspace.name}`}
                          onClick={() => handleRemove(workspace.id, workspace.name)}
                        >
                          Remove
                        </button>
                      </div>
                    </div>
                    <div className={styles.cardRoot} title={workspace.root}>
                      {workspace.root}
                    </div>
                  </article>
                </li>
              )
            })
          )}
        </ul>
      )}
    </div>
  )
}

function SearchIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden>
      <circle cx="11" cy="11" r="6.5" stroke="currentColor" strokeWidth="1.75" />
      <path d="M16 16.5 20 20.5" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" />
    </svg>
  )
}

function FolderMark() {
  return (
    <svg className={styles.mark} width="72" height="72" viewBox="0 0 72 72" fill="none" aria-hidden>
      <rect x="10" y="16" width="28" height="22" rx="3" stroke="currentColor" strokeWidth="1.5" />
      <rect x="22" y="26" width="28" height="22" rx="3" stroke="currentColor" strokeWidth="1.5" />
      <rect
        x="34"
        y="36"
        width="28"
        height="22"
        rx="3"
        stroke="currentColor"
        strokeWidth="1.5"
        fill="var(--bg-canvas)"
      />
    </svg>
  )
}
