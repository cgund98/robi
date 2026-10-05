import { cleanup, render, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { listMcpServers } from '../../api/mcp'
import { useMcpStore } from '../../state/mcpStore'
import { useWorkspaceStore } from '../../state/workspaceStore'
import { McpSettings } from './McpSettings'

vi.mock('../../api/mcp', () => ({
  focusMcp: vi.fn(async () => {}),
  getMcpConfig: vi.fn(async () => ({
    user_path: '/Users/ada/.robi/mcp.json',
    user_text: '{"mcpServers":{}}',
    project_path: '/Users/ada/work/.robi/mcp.json',
    project_text: null,
    project_enabled: false
  })),
  listMcpServers: vi.fn(async () => [])
}))

describe('McpSettings', () => {
  beforeEach(() => {
    useWorkspaceStore.setState({ activeWorkspaceId: 'w1' })
    useMcpStore.setState({ workspaceId: null, servers: [] })
  })

  afterEach(() => {
    cleanup()
    vi.mocked(listMcpServers).mockReset()
    useWorkspaceStore.setState({ activeWorkspaceId: null })
    useMcpStore.setState({ workspaceId: null, servers: [] })
  })

  it('lists known servers and points at their log directories', async () => {
    vi.mocked(listMcpServers).mockResolvedValue([
      { id: 'linear', status: 'connected', title: 'Linear', tool_count: 3, icon: null },
      { id: 'github', status: 'failed', title: null, tool_count: 0, icon: null }
    ])

    const { container } = render(<McpSettings />)

    await waitFor(() => expect(container.textContent).toContain('Linear'))
    // The status and tool count come from the same list the tray reads.
    expect(container.textContent).toContain('linear · connected · 3 tools')
    expect(container.textContent).toContain('github · failed')
    // The troubleshooting section names the log directory per server.
    expect(container.textContent).toContain('/Users/ada/.robi/logs/mcp')
    expect(container.textContent).toContain('/Users/ada/.robi/logs/mcp/linear/')
    expect(container.textContent).toContain('/Users/ada/.robi/logs/mcp/github/')
  })

  it('shows an empty state when no servers are configured', async () => {
    const { container } = render(<McpSettings />)

    await waitFor(() => expect(container.textContent).toContain('No servers yet'))
    // The log location still shows so a user can find it.
    expect(container.textContent).toContain('/Users/ada/.robi/logs/mcp')
  })
})
