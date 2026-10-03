import { createHashRouter, Navigate } from 'react-router-dom'

import { AppLayout } from '../components/layout/AppLayout'
import { WindowFrame } from '../components/layout/WindowFrame'
import { GeneralSettings } from '../pages/settings/GeneralSettings'
import { McpSettings } from '../pages/settings/McpSettings'
import { ModelProvidersSettings } from '../pages/settings/ModelProvidersSettings'
import { SettingsLayout } from '../pages/settings/SettingsLayout'
import { WorkspacesPage } from '../pages/workspaces/WorkspacesPage'

export const appRouter = createHashRouter([
  {
    element: <WindowFrame />,
    children: [
      {
        path: '/',
        element: <AppLayout />,
        children: [{ index: true }, { path: 'sessions/:sessionId/review' }]
      },
      {
        path: '/workspaces',
        element: <WorkspacesPage />
      },
      {
        path: '/settings',
        element: <SettingsLayout />,
        children: [
          { index: true, element: <Navigate to="providers" replace /> },
          { path: 'providers', element: <ModelProvidersSettings /> },
          { path: 'mcp', element: <McpSettings /> },
          { path: 'general', element: <GeneralSettings /> }
        ]
      }
    ]
  }
])
