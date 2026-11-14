import { describe, expect, it } from 'vitest'

import { projectServers } from './projectServers'

describe('projectServers', () => {
  it('lists a stdio entry as its command and args', () => {
    const text = JSON.stringify({
      mcpServers: {
        git: { command: 'npx', args: ['-y', '@modelcontextprotocol/server-git'] }
      }
    })
    expect(projectServers(text)).toEqual([
      { id: 'git', target: 'npx -y @modelcontextprotocol/server-git' }
    ])
  })

  it('lists an http entry as its url', () => {
    const text = JSON.stringify({
      mcpServers: { linear: { url: 'https://mcp.linear.app/mcp' } }
    })
    expect(projectServers(text)).toEqual([{ id: 'linear', target: 'https://mcp.linear.app/mcp' }])
  })

  it('leaves env and header values out of the target', () => {
    const text = JSON.stringify({
      mcpServers: {
        github: {
          command: 'github-mcp',
          args: [],
          env: { GITHUB_TOKEN: { secret: 'github_token' } }
        }
      }
    })
    const [server] = projectServers(text)
    expect(server.target).toBe('github-mcp')
    expect(JSON.stringify(server)).not.toContain('github_token')
  })

  it('keeps every entry, in file order', () => {
    const text = JSON.stringify({
      mcpServers: { one: { command: 'a' }, two: { command: 'b' } }
    })
    expect(projectServers(text).map((server) => server.id)).toEqual(['one', 'two'])
  })

  it('says so when the file is not JSON', () => {
    expect(projectServers('{ not json')).toEqual([
      { id: 'config', target: 'the file is not valid JSON' }
    ])
  })

  it('returns nothing for a missing file, empty text, or no mcpServers', () => {
    expect(projectServers(null)).toEqual([])
    expect(projectServers('')).toEqual([])
    expect(projectServers('{}')).toEqual([])
  })

  it('falls back to the id when an entry has neither command nor url', () => {
    expect(projectServers(JSON.stringify({ mcpServers: { odd: {} } }))).toEqual([
      { id: 'odd', target: 'odd' }
    ])
  })
})
