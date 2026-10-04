import { Outlet, useLocation } from 'react-router-dom'

import { useMouseHistory } from '../../app/mouseHistory'
import styles from './WindowFrame.module.css'

export function WindowFrame() {
  const { pathname } = useLocation()
  useMouseHistory()
  const tone = pathname.startsWith('/settings')
    ? styles.barSettings
    : pathname === '/' || pathname.startsWith('/sessions/') || pathname === '/docs'
      ? styles.barChat
      : styles.barPlain

  return (
    <div className={styles.frame}>
      <header className={`${styles.bar} ${tone}`} data-tauri-drag-region="deep" />
      <div className={styles.body}>
        <Outlet />
      </div>
    </div>
  )
}
