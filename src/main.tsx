import React from 'react'
import ReactDOM from 'react-dom/client'

import { resolveApiBase } from './api/client'
import { App } from './app/App'
import './styles/global.css'

void resolveApiBase().then(() => {
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  )
})
