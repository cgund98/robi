/** @jsxImportSource solid-js */
import type { JSX } from 'solid-js'
import { useLocation } from '@solidjs/router'

import styles from './WindowFrame.module.css'
import { useFrameChrome } from '../../app/useFrameChrome'

export function WindowFrame(props: { children?: JSX.Element }) {
  const location = useLocation()
  useFrameChrome()
  const tone = () => {
    const pathname = location.pathname
    if (pathname.startsWith('/settings')) {
      return styles.barSettings
    }
    if (pathname === '/' || pathname.startsWith('/sessions/') || pathname === '/docs') {
      return styles.barChat
    }
    return styles.barPlain
  }

  return (
    <div class={styles.frame}>
      <header class={`${styles.bar} ${tone()}`} data-tauri-drag-region="deep" />
      <div class={styles.body}>{props.children}</div>
    </div>
  )
}
