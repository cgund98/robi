import React from 'react'
import ReactDOM from 'react-dom/client'

import { resolveApiBase } from './api/client'
import { App } from './app/App'
import { withoutDocsRoute } from './app/startupRoute'
import { readUiRenderer } from './app/uiRenderer'
import { applyUiScale, useUiScaleStore } from './state/uiScaleStore'
import './styles/global.css'

void resolveApiBase().then(() => {
  // Apply the remembered UI scale before the first paint so the window does not
  // flash at 100%.
  void applyUiScale(useUiScaleStore.getState().scale).catch(() => {
    // The default scale is close enough to carry on with.
  })
  // Docs starts off. A `#/docs` hash kept from the last launch would reopen the
  // viewer; clear it before the router mounts.
  const hash = withoutDocsRoute(window.location.hash)
  if (hash !== window.location.hash) {
    window.history.replaceState(null, '', hash)
  }
  if (readUiRenderer() === 'solid') {
    document.getElementById('root')?.setAttribute('hidden', '')
    void import('./solid/main.tsx')
    return
  }
  document.getElementById('solid-root')?.setAttribute('hidden', '')
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
})
