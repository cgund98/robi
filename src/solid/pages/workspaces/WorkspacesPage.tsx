/** @jsxImportSource solid-js */
import { useNavigate } from '@solidjs/router'
import { Folder, MagnifyingGlass } from '../../ui/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'

import { pickWorkspaceRoot } from '../../../infra/pickWorkspaceRoot'
import styles from '../../../pages/workspaces/WorkspacesPage.module.css'
import { patchWorkspaces, workspaces } from '../../state/host'

export function WorkspacesPage() {
  const navigate = useNavigate()
  const [query, setQuery] = createSignal('')
  const [creating, setCreating] = createSignal(false)

  createEffect(() => {
    if (!workspaces.loaded) {
      void workspaces.loadWorkspaces()
    }
  })

  const visible = createMemo(() => {
    const needle = query().trim().toLowerCase()
    const list = workspaces.workspaces
    return needle
      ? list.filter(
          (workspace) =>
            workspace.name.toLowerCase().includes(needle) ||
            workspace.root.toLowerCase().includes(needle)
        )
      : list
  })

  async function handleNew() {
    setCreating(true)
    try {
      const root = await pickWorkspaceRoot()
      if (root == null) {
        return
      }
      await workspaces.addWorkspace(root)
      if (workspaces.error) {
        return
      }
      navigate('/')
    } catch (err) {
      patchWorkspaces({
        error: err instanceof Error ? err.message : 'Failed to open the folder dialog'
      })
    } finally {
      setCreating(false)
    }
  }

  function openWorkspace(id: string) {
    workspaces.selectWorkspace(id)
    navigate('/')
  }

  function handleRemove(id: string, name: string) {
    if (!window.confirm(`Remove “${name}”? Its sessions will be deleted.`)) {
      return
    }
    void workspaces.removeWorkspace(id)
  }

  const empty = () => workspaces.loaded && workspaces.workspaces.length === 0

  return (
    <div class={styles.page}>
      <div class={styles.column}>
        <header class={styles.top}>
          <div class={styles.heading}>
            <Show when={workspaces.activeWorkspaceId}>
              <button type="button" class={styles.back} onClick={() => navigate('/')}>
                ← Chat
              </button>
            </Show>
            <h1 class={styles.title}>Workspaces</h1>
          </div>
          <div class={styles.tools}>
            <label class={styles.search}>
              <MagnifyingGlass size={14} aria-hidden />
              <input
                value={query()}
                onInput={(event) => setQuery(event.currentTarget.value)}
                placeholder="Search"
                aria-label="Search workspaces"
              />
            </label>
            <button
              type="button"
              class={styles.primary}
              onClick={() => void handleNew()}
              disabled={creating()}
            >
              New workspace
            </button>
          </div>
        </header>

        <Show when={workspaces.error}>
          <div class={styles.banner} role="alert">
            {workspaces.error}
          </div>
        </Show>

        <Show when={workspaces.loaded} fallback={<p class={styles.status}>Loading…</p>}>
          <Show
            when={!empty()}
            fallback={
              <div class={styles.empty}>
                <Folder class={styles.mark} size={72} aria-hidden />
                <h2 class={styles.emptyTitle}>Looking to start a workspace?</h2>
                <p class={styles.emptyCopy}>
                  Open a directory. Sessions stay with that folder, and the agent works from its
                  root.
                </p>
                <button
                  type="button"
                  class={styles.emptyAction}
                  onClick={() => void handleNew()}
                  disabled={creating()}
                >
                  New workspace
                </button>
              </div>
            }
          >
            <ul class={styles.list}>
              <Show
                when={visible().length > 0}
                fallback={<li class={styles.noMatch}>No workspaces match “{query().trim()}”.</li>}
              >
                <For each={visible()}>
                  {(workspace) => {
                    const current = () => workspace.id === workspaces.activeWorkspaceId
                    return (
                      <li>
                        <article class={styles.card}>
                          <div class={styles.cardHead}>
                            <button
                              type="button"
                              class={styles.cardName}
                              onClick={() => openWorkspace(workspace.id)}
                            >
                              {workspace.name}
                              <Show when={current()}>
                                <span class={styles.current}>Current</span>
                              </Show>
                            </button>
                            <div class={styles.actions}>
                              <button
                                type="button"
                                class={styles.open}
                                onClick={() => openWorkspace(workspace.id)}
                              >
                                Open
                              </button>
                              <button
                                type="button"
                                class={styles.remove}
                                aria-label={`Remove ${workspace.name}`}
                                onClick={() => handleRemove(workspace.id, workspace.name)}
                              >
                                Remove
                              </button>
                            </div>
                          </div>
                          <div class={styles.cardRoot} title={workspace.root}>
                            {workspace.root}
                          </div>
                        </article>
                      </li>
                    )
                  }}
                </For>
              </Show>
            </ul>
          </Show>
        </Show>
      </div>
    </div>
  )
}
