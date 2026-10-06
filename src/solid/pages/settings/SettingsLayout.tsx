/** @jsxImportSource solid-js */
import { A } from '@solidjs/router'
import type { JSX } from 'solid-js'
import { For } from 'solid-js'

import styles from '../../../pages/settings/Settings.module.css'

const ITEMS = [
  { to: '/settings/audit', label: 'Audit log' },
  { to: '/settings/general', label: 'General' },
  { to: '/settings/mcp', label: 'MCP' },
  { to: '/settings/providers', label: 'Model Providers' },
  { to: '/settings/permissions', label: 'Permissions' }
] as const

export function SettingsLayout(props: { children?: JSX.Element }) {
  return (
    <div class={styles.page}>
      <nav class={styles.nav} aria-label="Settings">
        <A class={styles.back} href="/">
          ← Back
        </A>
        <div class={styles.sectionLabel}>Settings</div>
        <For each={ITEMS}>
          {(item) => (
            <A class={styles.item} activeClass={styles.itemActive} href={item.to} end>
              {item.label}
            </A>
          )}
        </For>
      </nav>
      <main class={styles.main}>
        <div class={styles.column}>{props.children}</div>
      </main>
    </div>
  )
}
