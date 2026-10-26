import { HashRouter, Navigate, Route, useParams } from '@solidjs/router'

import { ChatChrome } from '../components/layout/ChatChrome'
import { WindowFrame } from '../components/layout/WindowFrame'
import { AuditLogSettings } from '../features/settings/AuditLogSettings'
import { GeneralSettings } from '../features/settings/GeneralSettings'
import { McpSettings } from '../features/settings/McpSettings'
import { ModelProvidersSettings } from '../features/settings/ModelProvidersSettings'
import { PermissionsSettings } from '../features/settings/PermissionsSettings'
import { SandboxSettings } from '../features/settings/SandboxSettings'
import { SettingsLayout } from '../features/settings/SettingsLayout'
import { WorkspacesPage } from '../features/workspaces/WorkspacesPage'
import { chatRoutePaths } from './routes'
import { showSurface } from './surfaces'

/**
 * A review hash kept from an older build, or from before the screen was
 * removed. It lands on that session's chat.
 */
function ReviewRedirect() {
  const params = useParams<{ sessionId: string }>()
  return <Navigate href={`/sessions/${params.sessionId}`} />
}

export function SolidApp() {
  return (
    <HashRouter root={WindowFrame}>
      <Route path={chatRoutePaths(showSurface)} component={ChatChrome} />
      {!showSurface('reviewScreen') && (
        <Route path="/sessions/:sessionId/review" component={ReviewRedirect} />
      )}
      <Route path="/workspaces" component={WorkspacesPage} />
      <Route path="/settings" component={SettingsLayout}>
        <Route path="/" component={() => <Navigate href="/settings/general" />} />
        <Route path="/providers" component={ModelProvidersSettings} />
        <Route path="/mcp" component={McpSettings} />
        <Route path="/permissions" component={PermissionsSettings} />
        <Route path="/sandbox" component={SandboxSettings} />
        <Route path="/general" component={GeneralSettings} />
        <Route path="/audit" component={AuditLogSettings} />
      </Route>
    </HashRouter>
  )
}
