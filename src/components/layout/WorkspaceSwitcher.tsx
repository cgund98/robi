import { ChevronDown, Folder, FolderOpen } from 'lucide-react'
import { useState } from 'react'
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'

import { pickWorkspaceRoot } from '../../infra/pickWorkspaceRoot'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './WorkspaceSwitcher.module.css'

export function WorkspaceSwitcher() {
  const [open, setOpen] = useState(false)
  const workspaces = useWorkspaceStore((state) => state.workspaces)
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const selectWorkspace = useWorkspaceStore((state) => state.selectWorkspace)
  const addWorkspace = useWorkspaceStore((state) => state.addWorkspace)
  const active = workspaces.find((workspace) => workspace.id === activeWorkspaceId) ?? null

  async function handleAdd() {
    try {
      const root = await pickWorkspaceRoot()
      if (root == null) {
        return
      }
      await addWorkspace(root)
    } catch (err) {
      useWorkspaceStore.setState({
        error: err instanceof Error ? err.message : 'Failed to open the folder dialog'
      })
    }
  }

  return (
    <div className={styles.switcher}>
      <DropdownMenu.Root open={open} onOpenChange={setOpen}>
        <DropdownMenu.Trigger className={styles.summary}>
          <FolderIcon open={active !== null} />
          <span className={styles.name}>{active?.name ?? 'Choose a workspace'}</span>
          <span className={styles.chevron} aria-hidden>
            <ChevronDown size={12} strokeWidth={1.5} />
          </span>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className={styles.panel} side="bottom" align="start" sideOffset={8}>
            <DropdownMenu.RadioGroup
              value={activeWorkspaceId ?? ''}
              onValueChange={selectWorkspace}
            >
              {workspaces.map((workspace) => (
                <DropdownMenu.RadioItem
                  key={workspace.id}
                  value={workspace.id}
                  className={styles.item}
                  title={workspace.root}
                >
                  <FolderIcon open={workspace.id === activeWorkspaceId} />
                  <span className={styles.itemCopy}>
                    <span className={styles.itemName}>{workspace.name}</span>
                    <span className={styles.itemRoot}>{workspace.root}</span>
                  </span>
                </DropdownMenu.RadioItem>
              ))}
            </DropdownMenu.RadioGroup>
            <DropdownMenu.Item className={styles.action} onSelect={() => void handleAdd()}>
              Add workspace…
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  )
}

function FolderIcon({ open = false }: { open?: boolean }) {
  const Icon = open ? FolderOpen : Folder
  return <Icon className={styles.folder} size={14} strokeWidth={1.75} aria-hidden />
}
