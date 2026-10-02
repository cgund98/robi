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
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path
                d="M2.5 4.5 6 8l3.5-3.5"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
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
  return (
    <svg
      className={styles.folder}
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden
    >
      {open ? (
        <path
          d="M4 20h14.4a2 2 0 0 0 1.94-1.52L22 11H6.2a2 2 0 0 0-1.9 1.37L2.2 19.5A1.2 1.2 0 0 0 4 20ZM4 20V6.5A1.5 1.5 0 0 1 5.5 5H9l2 2h6.5A1.5 1.5 0 0 1 19 8.5V11"
          stroke="currentColor"
          strokeWidth="1.75"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ) : (
        <path
          d="M3.5 7.5A2 2 0 0 1 5.5 5.5h4l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2v-10Z"
          stroke="currentColor"
          strokeWidth="1.75"
          strokeLinejoin="round"
        />
      )}
    </svg>
  )
}
