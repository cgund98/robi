/** @jsxImportSource solid-js */
import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { ChevronDown, Folder, FolderOpen } from '../ui/icons'
import { For } from 'solid-js'

import { pickWorkspaceRoot } from '../../infra/pickWorkspaceRoot'
import styles from './WorkspaceSwitcher.module.css'
import { patchWorkspaces, workspaces } from '../../state/workspaceStore'

export function WorkspaceSwitcher() {
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
              <For each={workspaces.workspaces}>
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
