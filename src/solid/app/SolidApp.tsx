/** @jsxImportSource solid-js */
import { HashRouter, Navigate, Route } from '@solidjs/router'

import { ChatChrome } from '../components/layout/ChatChrome'
import { WindowFrame } from '../components/layout/WindowFrame'
import { AuditLogSettings } from '../pages/settings/AuditLogSettings'
import { GeneralSettings } from '../pages/settings/GeneralSettings'
import { McpSettings } from '../pages/settings/McpSettings'
import { ModelProvidersSettings } from '../pages/settings/ModelProvidersSettings'
import { PermissionsSettings } from '../pages/settings/PermissionsSettings'
import { SettingsLayout } from '../pages/settings/SettingsLayout'
import { WorkspacesPage } from '../pages/workspaces/WorkspacesPage'

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
