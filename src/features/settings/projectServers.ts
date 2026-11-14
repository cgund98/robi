// The ids and targets a project MCP file declares, for the trust panel.
//
// Only `command` plus `args`, or `url`. Environment and header values stay out
// of the list even though the whole file is printed lower on the page.

export type ProjectServer = { id: string; target: string }

export function projectServers(text?: string | null): ProjectServer[] {
  if (!text) {
    return []
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch {
    return [{ id: 'config', target: 'the file is not valid JSON' }]
  }
  const servers = (parsed as { mcpServers?: Record<string, unknown> }).mcpServers
  if (!servers || typeof servers !== 'object') {
    return []
  }
  return Object.entries(servers).map(([id, body]) => {
    const entry = (body ?? {}) as { command?: unknown; args?: unknown; url?: unknown }
    const args = Array.isArray(entry.args)
      ? entry.args.filter((arg): arg is string => typeof arg === 'string')
      : []
    const command = typeof entry.command === 'string' ? entry.command : ''
    const url = typeof entry.url === 'string' ? entry.url : ''
    const target = command ? [command, ...args].join(' ') : url || id
    return { id, target }
  })
}
