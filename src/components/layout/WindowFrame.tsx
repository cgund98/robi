import type { JSX } from 'solid-js'
import { useLocation } from '@solidjs/router'

import styles from './WindowFrame.module.css'
import { isMacOs } from '../../app/platform'
import { showSurface } from '../../app/surfaces'
import { useFrameChrome } from '../../app/useFrameChrome'

export function WindowFrame(props: { children?: JSX.Element }) {
  const location = useLocation()
  useFrameChrome()
  const tone = () => {
    const pathname = location.pathname
    if (pathname.startsWith('/settings')) {
      return styles.barSettings
    }
    // The top bar spans the window, so the chat shell's strip is plain canvas
    // there. Only the legacy sidebar layout continues its column into the bar.
    if (!showSurface('topBarShell') && (pathname === '/' || pathname.startsWith('/sessions/'))) {
      return styles.barChat
    }
    return styles.barPlain
  }

  return (
    <div class={styles.frame} data-os={isMacOs() ? 'macos' : undefined}>
      <header class={`${styles.bar} ${tone()}`} data-tauri-drag-region="deep" />
      <div class={styles.body}>{props.children}</div>
    </div>
  )
}
