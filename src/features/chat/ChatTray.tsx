/** @jsxImportSource solid-js */
import { ChatBubble, XMark } from '../../components/ui/icons'
import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

import {
  clampTrayWidth,
  readTrayWidth,
  TRAY_MIN_WIDTH,
  trayMaxWidth,
  writeTrayWidth
} from './trayWidth'
import styles from './ChatTray.module.css'
import { ChatPanel, type ChatPanelProps } from './ChatPanel'

const PANEL_ID = 'docs-chat-tray'
const STEP = 24

/**
 * Width of the column the tray overlays. The 320px kept clear is that column,
 * so the sidebar does not count toward it and the file tree stays clickable.
 */
function columnWidth(anchor: HTMLElement | undefined): number {
  const column = anchor?.offsetParent
  if (column instanceof HTMLElement && column.clientWidth > 0) {
    return column.clientWidth
  }
  return typeof window === 'undefined' ? TRAY_MIN_WIDTH : window.innerWidth
}

export function ChatTray(
  props: ChatPanelProps & {
    open: boolean
    onOpen: () => void
    onClose: () => void
    title: string
  }
) {
  let anchor: HTMLElement | undefined
  const setAnchor = (el: HTMLElement) => {
    anchor = el
  }
  const [width, setWidth] = createSignal(readTrayWidth(columnWidth(undefined)))
  const [dragging, setDragging] = createSignal(false)
  let drag: { pointerId: number; startX: number; startWidth: number } | null = null

  const fit = () => setWidth((current) => clampTrayWidth(current, columnWidth(anchor)))

  createEffect(() => {
    if (!props.open) {
      return
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        props.onClose()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })

  createEffect(() => {
    writeTrayWidth(width())
  })

  createEffect(() => {
    void props.open
    fit()
  })

  createEffect(() => {
    const onResize = () => fit()
    window.addEventListener('resize', onResize)
    onCleanup(() => window.removeEventListener('resize', onResize))
  })

  createEffect(() => {
    if (!dragging()) {
      return
    }
    const onMove = (event: PointerEvent) => {
      const active = drag
      if (!active || event.pointerId !== active.pointerId) {
        return
      }
      setWidth(
        clampTrayWidth(active.startWidth + (active.startX - event.clientX), columnWidth(anchor))
      )
    }
    const onUp = (event: PointerEvent) => {
      if (drag && event.pointerId !== drag.pointerId) {
        return
      }
      drag = null
      setDragging(false)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onUp)
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'col-resize'
    onCleanup(() => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onUp)
      document.body.style.userSelect = ''
      document.body.style.cursor = ''
    })
  })

  return (
    <Show
      when={props.open}
      fallback={
        <button
          type="button"
          ref={setAnchor}
          class={styles.handle}
          aria-expanded={false}
          aria-controls={PANEL_ID}
          aria-label="Show chat"
          title="Show chat"
          onClick={() => props.onOpen()}
        >
          <ChatBubble size={18} aria-hidden="true" />
        </button>
      }
    >
      <aside
        id={PANEL_ID}
        ref={setAnchor}
        class={styles.tray}
        role="complementary"
        aria-label="Chat"
        style={{ width: `${width()}px` }}
      >
        <div
          class={`${styles.resizer} ${dragging() ? styles.resizing : ''}`}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize chat tray"
          aria-valuenow={Math.round(width())}
          aria-valuemin={TRAY_MIN_WIDTH}
          aria-valuemax={Math.round(trayMaxWidth(columnWidth(anchor)))}
          tabIndex={0}
          onPointerDown={(event) => {
            drag = { pointerId: event.pointerId, startX: event.clientX, startWidth: width() }
            event.currentTarget.setPointerCapture(event.pointerId)
            setDragging(true)
          }}
          onKeyDown={(event) => {
            const max = trayMaxWidth(columnWidth(anchor))
            if (event.key === 'ArrowLeft') {
              event.preventDefault()
              setWidth((current) => clampTrayWidth(current + STEP, columnWidth(anchor)))
            } else if (event.key === 'ArrowRight') {
              event.preventDefault()
              setWidth((current) => clampTrayWidth(current - STEP, columnWidth(anchor)))
            } else if (event.key === 'Home') {
              event.preventDefault()
              setWidth(clampTrayWidth(TRAY_MIN_WIDTH, columnWidth(anchor)))
            } else if (event.key === 'End') {
              event.preventDefault()
              setWidth(max)
            }
          }}
        />
        <header class={styles.header}>
          <span class={styles.title}>{props.title}</span>
          <button
            type="button"
            class={styles.close}
            aria-label="Close chat"
            title="Close chat"
            onClick={() => props.onClose()}
          >
            <XMark size={16} aria-hidden="true" />
          </button>
        </header>
        <ChatPanel {...props} />
      </aside>
    </Show>
  )
}
