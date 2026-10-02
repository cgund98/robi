import { createHashRouter, Navigate } from 'react-router-dom'

import { AppLayout } from '../components/layout/AppLayout'
import { GeneralSettings } from '../pages/settings/GeneralSettings'
import { ModelProvidersSettings } from '../pages/settings/ModelProvidersSettings'
import { SettingsLayout } from '../pages/settings/SettingsLayout'
import { WorkspacesPage } from '../pages/workspaces/WorkspacesPage'

export const appRouter = createHashRouter([
  {
    path: '/',
    element: <AppLayout />
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
      { path: 'general', element: <GeneralSettings /> }
    ]
  }
])
