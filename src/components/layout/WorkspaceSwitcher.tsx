import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { useNavigate } from '@solidjs/router'
import { ChevronDown, Folder, FolderOpen } from '../ui/icons'
import { For, Show } from 'solid-js'

import { pickWorkspaceRoot } from '../../infra/pickWorkspaceRoot'
import styles from './WorkspaceSwitcher.module.css'
import { orderByRecentUse, patchWorkspaces, workspaces } from '../../state/workspaceStore'

const MENU_LIMIT = 8

export function WorkspaceSwitcher() {
  const navigate = useNavigate()
  const ordered = () => orderByRecentUse(workspaces.workspaces, workspaces.recentWorkspaceIds)
  const active = () =>
    workspaces.workspaces.find((workspace) => workspace.id === workspaces.activeWorkspaceId) ?? null

  async function handleAdd() {
    try {
      const root = await pickWorkspaceRoot()
      if (root == null) {
        return
      }
      await workspaces.addWorkspace(root)
    } catch (err) {
      patchWorkspaces({
        error: err instanceof Error ? err.message : 'Failed to open the folder dialog'
      })
    }
  }

  return (
    <div class={styles.switcher}>
      <DropdownMenu>
        <DropdownMenu.Trigger class={styles.summary}>
          <FolderIcon open={active() !== null} />
          <span class={styles.name}>{active()?.name ?? 'Choose a workspace'}</span>
          <span class={styles.chevron} aria-hidden="true">
            <ChevronDown size={12} />
          </span>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content class={styles.panel}>
            <DropdownMenu.RadioGroup
              value={workspaces.activeWorkspaceId ?? ''}
              onChange={(id) => void workspaces.selectWorkspace(id)}
            >
              <For each={ordered().slice(0, MENU_LIMIT)}>
                {(workspace) => (
                  <DropdownMenu.RadioItem
                    value={workspace.id}
                    class={styles.item}
                    title={workspace.root}
                  >
                    <FolderIcon open={workspace.id === workspaces.activeWorkspaceId} />
                    <span class={styles.itemCopy}>
                      <span class={styles.itemName}>{workspace.name}</span>
                      <span class={styles.itemRoot}>{workspace.root}</span>
                    </span>
                  </DropdownMenu.RadioItem>
                )}
              </For>
            </DropdownMenu.RadioGroup>
            <Show when={ordered().length > MENU_LIMIT}>
              <DropdownMenu.Item
                class={styles.action}
                as="a"
                href="#/workspaces"
                onSelect={() => navigate('/workspaces')}
              >
                All workspaces…
              </DropdownMenu.Item>
            </Show>
            <DropdownMenu.Item class={styles.action} onSelect={() => void handleAdd()}>
              Add workspace…
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu>
    </div>
  )
}

function FolderIcon(props: { open?: boolean }) {
  return props.open ? (
    <FolderOpen class={styles.folder} size={14} aria-hidden="true" />
  ) : (
    <Folder class={styles.folder} size={14} aria-hidden="true" />
  )
}
