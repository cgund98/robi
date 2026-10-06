/** @jsxImportSource solid-js */
import { render } from 'solid-js/web'
import { describe, expect, it, vi } from 'vitest'

import { DocLineMenu } from './DocLineMenu'

function mount(props: {
  onAddToChat?: () => void
  onOpenInEditor?: () => void
  onOpenChange?: (open: boolean) => void
}) {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const dispose = render(
    () => (
      <DocLineMenu
        top={12}
        label="Line actions for lines 4 to 6"
        onAddToChat={props.onAddToChat}
        onOpenInEditor={props.onOpenInEditor ?? (() => {})}
        onOpenChange={props.onOpenChange ?? (() => {})}
      />
    ),
    container
  )
  return { container, dispose }
}

function trigger(container: HTMLElement): HTMLElement {
  const node = container.querySelector<HTMLElement>('[data-md-attach]')
  if (!node) {
    throw new Error('trigger not rendered')
  }
  return node
}

/** Open a Kobalte menu. The trigger responds to a pointerdown. */
async function open(node: HTMLElement) {
  node.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true }))
  await Promise.resolve()
  await Promise.resolve()
}

describe('DocLineMenu', () => {
  it('renders an ellipsis trigger with the block label', () => {
    const { container, dispose } = mount({})
    const node = trigger(container)
    expect(node.querySelector('svg')).not.toBeNull()
    expect(node.getAttribute('aria-label')).toBe('Line actions for lines 4 to 6')
    dispose()
  })

  it('offers Open in editor alone when nothing can take a chat line', async () => {
    const { container, dispose } = mount({})
    await open(trigger(container))
    const items = [...document.querySelectorAll('[role="menuitem"]')]
    expect(items.map((item) => item.textContent)).toEqual(['Open in editor'])
    dispose()
  })

  it('offers both actions when the tray can take a chat line', async () => {
    const { container, dispose } = mount({ onAddToChat: () => {} })
    await open(trigger(container))
    const items = [...document.querySelectorAll('[role="menuitem"]')]
    expect(items.map((item) => item.textContent)).toEqual(['Add to chat', 'Open in editor'])
    dispose()
  })

  it('runs Open in editor when its item is selected', async () => {
    const onOpenInEditor = vi.fn()
    const { container, dispose } = mount({ onOpenInEditor })
    await open(trigger(container))
    const item = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(
      (node) => node.textContent === 'Open in editor'
    )
    // Kobalte selects on a press, not a bare click.
    item?.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true }))
    item?.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, cancelable: true }))
    await Promise.resolve()
    expect(onOpenInEditor).toHaveBeenCalledTimes(1)
    dispose()
  })
})
