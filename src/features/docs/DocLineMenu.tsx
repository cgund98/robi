/** @jsxImportSource solid-js */
import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { Show } from 'solid-js'

import { EllipsisHorizontal } from '../../components/ui/icons'
import styles from './DocLineMenu.module.css'

/**
 * The actions for one hovered block of a rendered page.
 *
 * The trigger is pinned to the block's top inside the sheet. `data-md-attach`
 * and `data-find-ignore` keep the viewer's hover tracking and the find bar off
 * it. "Add to chat" is only offered when the screen has a chat tray to receive
 * the line.
 */
export function DocLineMenu(props: {
  top: number
  label: string
  onAddToChat?: () => void
  onOpenInEditor: () => void
  /** The menu opened or closed. The viewer keeps the block hovered while open. */
  onOpenChange: (open: boolean) => void
}) {
  return (
    <DropdownMenu onOpenChange={props.onOpenChange}>
      <DropdownMenu.Trigger
        class={styles.trigger}
        style={{ top: `${props.top}px` }}
        aria-label={props.label}
        title="Line actions"
        data-md-attach
        data-find-ignore
      >
        <EllipsisHorizontal size={16} aria-hidden="true" />
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class={styles.panel}>
          <Show when={props.onAddToChat}>
            <DropdownMenu.Item class={styles.item} onSelect={() => props.onAddToChat?.()}>
              Add to chat
            </DropdownMenu.Item>
          </Show>
          <DropdownMenu.Item class={styles.item} onSelect={() => props.onOpenInEditor()}>
            Open in editor
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu>
  )
}
