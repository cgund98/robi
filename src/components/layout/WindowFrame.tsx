import { Outlet, useLocation } from 'react-router-dom'

import styles from './WindowFrame.module.css'

export function WindowFrame() {
  const { pathname } = useLocation()
  const tone = pathname.startsWith('/settings')
    ? styles.barSettings
    : pathname === '/' || pathname.startsWith('/sessions/')
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
