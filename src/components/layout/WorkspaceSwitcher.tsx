import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { useNavigate } from '@solidjs/router'
import { ChevronDown, Folder, FolderOpen } from '../ui/icons'
import { For } from 'solid-js'

import styles from './WorkspaceSwitcher.module.css'
import { orderByRecentUse, workspaces } from '../../state/workspaceStore'

const MENU_LIMIT = 8

/**
 * The active workspace, with a dropdown of the others.
 *
 * `compact` is the top-bar form: an inline chip instead of the full-width row
 * the sidebar used. The menu itself is the same in both. Adding a workspace is
 * the workspaces page's job, so this menu switches and nothing more.
 */
export function WorkspaceSwitcher(props: { compact?: boolean } = {}) {
  const navigate = useNavigate()
  const ordered = () => orderByRecentUse(workspaces.workspaces, workspaces.recentWorkspaceIds)
  const active = () =>
    workspaces.workspaces.find((workspace) => workspace.id === workspaces.activeWorkspaceId) ?? null

  return (
    <div class={props.compact ? `${styles.switcher} ${styles.switcherCompact}` : styles.switcher}>
      <DropdownMenu>
        <DropdownMenu.Trigger
          class={props.compact ? `${styles.summary} ${styles.summaryCompact}` : styles.summary}
          aria-label={`Workspace: ${active()?.name ?? 'choose one'}`}
        >
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
            <DropdownMenu.Separator class={styles.separator} />
            <DropdownMenu.Item
              class={styles.action}
              as="a"
              href="#/workspaces"
              onSelect={() => navigate('/workspaces')}
            >
              Manage workspaces…
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
