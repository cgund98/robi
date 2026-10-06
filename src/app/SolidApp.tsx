/** @jsxImportSource solid-js */
import { HashRouter, Navigate, Route } from '@solidjs/router'

import { ChatChrome } from '../components/layout/ChatChrome'
import { WindowFrame } from '../components/layout/WindowFrame'
import { AuditLogSettings } from '../features/settings/AuditLogSettings'
import { GeneralSettings } from '../features/settings/GeneralSettings'
import { McpSettings } from '../features/settings/McpSettings'
import { ModelProvidersSettings } from '../features/settings/ModelProvidersSettings'
import { PermissionsSettings } from '../features/settings/PermissionsSettings'
import { SettingsLayout } from '../features/settings/SettingsLayout'
import { WorkspacesPage } from '../features/workspaces/WorkspacesPage'

export function SolidApp() {
  return (
    <HashRouter root={WindowFrame}>
      <Route
        path={['/', '/sessions/:sessionId', '/sessions/:sessionId/review', '/docs']}
        component={ChatChrome}
      />
      <Route path="/workspaces" component={WorkspacesPage} />
      <Route path="/settings" component={SettingsLayout}>
        <Route path="/" component={() => <Navigate href="/settings/providers" />} />
        <Route path="/providers" component={ModelProvidersSettings} />
        <Route path="/mcp" component={McpSettings} />
        <Route path="/permissions" component={PermissionsSettings} />
        <Route path="/general" component={GeneralSettings} />
        <Route path="/audit" component={AuditLogSettings} />
      </Route>
    </HashRouter>
  )
}
