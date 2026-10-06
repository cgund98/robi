/** @jsxImportSource solid-js */
import { render } from 'solid-js/web'

import { resolveApiBase } from './api/client'
import { withoutDocsRoute } from './app/startupRoute'
import { applyUiScale, uiScale } from './state/uiScaleStore'
import './styles/global.css'
import { SolidApp } from './app/SolidApp'

const root = document.getElementById('root')
if (!root) {
  throw new Error('Missing #root')
}

void resolveApiBase().then(() => {
  void applyUiScale(uiScale.scale).catch(() => {
    // The default scale is close enough to carry on with.
  })
  const hash = withoutDocsRoute(window.location.hash)
  if (hash !== window.location.hash) {
    window.history.replaceState(null, '', hash)
  }
  render(() => <SolidApp />, root)
})
