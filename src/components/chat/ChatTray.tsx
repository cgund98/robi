import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'
import { MessageSquare, X } from 'lucide-react'

import { ChatPanel, type ChatPanelProps } from './ChatPanel'
import {
  clampTrayWidth,
  readTrayWidth,
  TRAY_MIN_WIDTH,
  trayMaxWidth,
  writeTrayWidth
} from './trayWidth'
import styles from './ChatTray.module.css'

const PANEL_ID = 'docs-chat-tray'
/** One arrow-key step. */
const STEP = 24

type ChatTrayProps = ChatPanelProps & {
  open: boolean
  onOpen: () => void
  onClose: () => void
  /** Session title shown in the tray header. */
  title: string
}

function viewportWidth(): number {
  return typeof window === 'undefined' ? TRAY_MIN_WIDTH : window.innerWidth
}

/**
 * The right-side chat tray on the documentation view. Closed by default: a
 * slim handle sits on the right edge until it is opened. The panel holds the
 * same chat surface as the main column, and its left edge is a drag handle
 * that resizes it.
 */
export function ChatTray({ open, onOpen, onClose, title, ...chat }: ChatTrayProps) {
  const [width, setWidth] = useState(() => readTrayWidth(viewportWidth()))
  const [dragging, setDragging] = useState(false)
  // The pointer that started the drag, and the width when it started.
  const drag = useRef<{ pointerId: number; startX: number; startWidth: number } | null>(null)

  useEffect(() => {
    if (!open) {
      return
    }
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === 'Escape') {
        onClose()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [open, onClose])

  // Remember the width across visits.
  useEffect(() => {
    writeTrayWidth(width)
  }, [width])

  // A narrower viewport can leave a stored width too wide.
  useEffect(() => {
    function onResize() {
      setWidth((current) => clampTrayWidth(current, viewportWidth()))
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  useEffect(() => {
    if (!dragging) {
      return
    }
    function onMove(event: globalThis.PointerEvent) {
      const active = drag.current
      if (!active || event.pointerId !== active.pointerId) {
        return
      }
      // The handle is on the left edge, so dragging left widens the tray.
      setWidth(clampTrayWidth(active.startWidth + (active.startX - event.clientX), viewportWidth()))
    }
    function onUp(event: globalThis.PointerEvent) {
      if (drag.current && event.pointerId !== drag.current.pointerId) {
        return
      }
      drag.current = null
      setDragging(false)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onUp)
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'col-resize'
    return () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onUp)
      document.body.style.userSelect = ''
      document.body.style.cursor = ''
    }
  }, [dragging])

  function onHandlePointerDown(event: PointerEvent<HTMLDivElement>) {
    drag.current = { pointerId: event.pointerId, startX: event.clientX, startWidth: width }
    event.currentTarget.setPointerCapture(event.pointerId)
    setDragging(true)
  }

  function onHandleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const max = trayMaxWidth(viewportWidth())
    if (event.key === 'ArrowLeft') {
      event.preventDefault()
      setWidth((current) => clampTrayWidth(current + STEP, viewportWidth()))
    } else if (event.key === 'ArrowRight') {
      event.preventDefault()
      setWidth((current) => clampTrayWidth(current - STEP, viewportWidth()))
    } else if (event.key === 'Home') {
      event.preventDefault()
      setWidth(clampTrayWidth(TRAY_MIN_WIDTH, viewportWidth()))
    } else if (event.key === 'End') {
      event.preventDefault()
      setWidth(max)
    }
  }

  if (!open) {
    return (
      <button
        type="button"
        className={styles.handle}
        aria-expanded={false}
        aria-controls={PANEL_ID}
        aria-label="Show chat"
        title="Show chat"
        onClick={onOpen}
      >
        <MessageSquare size={18} strokeWidth={1.75} aria-hidden />
      </button>
    )
  }

  return (
    <aside
      id={PANEL_ID}
      className={styles.tray}
      role="complementary"
      aria-label="Chat"
      style={{ width: `${width}px` }}
    >
      <div
        className={`${styles.resizer} ${dragging ? styles.resizing : ''}`}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize chat tray"
        aria-valuenow={Math.round(width)}
        aria-valuemin={TRAY_MIN_WIDTH}
        aria-valuemax={Math.round(trayMaxWidth(viewportWidth()))}
        tabIndex={0}
        onPointerDown={onHandlePointerDown}
        onKeyDown={onHandleKeyDown}
      />
      <header className={styles.header}>
        <span className={styles.title}>{title}</span>
        <button
          type="button"
          className={styles.close}
          aria-label="Close chat"
          title="Close chat"
          onClick={onClose}
        >
          <X size={16} strokeWidth={1.75} aria-hidden />
        </button>
      </header>
      <ChatPanel {...chat} />
    </aside>
  )
}
