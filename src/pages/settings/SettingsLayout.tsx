import { NavLink, Outlet } from 'react-router-dom'

import styles from './Settings.module.css'

const ITEMS = [
  { to: '/settings/providers', label: 'Model Providers' },
  { to: '/settings/mcp', label: 'MCP' },
  { to: '/settings/general', label: 'General' }
] as const

export function SettingsLayout() {
  return (
    <div className={styles.page}>
      <nav className={styles.nav} aria-label="Settings">
        <NavLink className={styles.back} to="/">
          ← Back to app
        </NavLink>
        <div className={styles.sectionLabel}>Settings</div>
        {ITEMS.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            className={({ isActive }) => `${styles.item} ${isActive ? styles.itemActive : ''}`}
          >
            {item.label}
          </NavLink>
        ))}
      </nav>
      <main className={styles.main}>
        <Outlet />
      </main>
    </div>
  )
}
