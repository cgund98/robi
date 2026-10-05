# MCP

The **Model Context Protocol** (MCP) lets Robi use tools that other programs
provide. Robi is an MCP **host** (client): it connects to servers you configure,
and each remote tool becomes a tool the assistant can call. Robi does not accept
MCP connections from anyone.

MCP tools are registered in **agent** mode only. Ask and plan keep their built-in
tool sets, and subagents never receive MCP tools.

## Configuring a server

Two JSON files share one shape:

- `~/.robi/mcp.json` — your servers.
- `<workspace>/.robi/mcp.json` — servers for one project.

A server has a `command` (stdio) **or** a `url` (HTTP), not both. Environment
values and HTTP headers may be a plain string or `{"secret": "<name>"}`, where
`<name>` is read from `~/.robi/secrets.toml` and never written back.

```json
{
  "mcpServers": {
    "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] }
  }
}
```

Details that are enforced:

- A server id matches `^[A-Za-z0-9_-]{1,32}$`.
- `enabled: false` skips a server.
- `command` is an executable name or an absolute path; a relative path is a
  config error.
- `url` must be `https`, or `http` on `127.0.0.1` or `localhost`.
- `timeout_seconds` defaults to 60; a value over 300 is a config error.
- Unknown fields are ignored.

The config is read when the workspace becomes the open one, and again on an
agent-mode turn for a server that is not already connected. An edit applies
the next time that workspace is focused, or on the next message if the
server is not up yet. Restart the app if you want the change picked up right
away.

The bundled `configure-mcp` skill walks you through adding an entry — ask to add
an MCP server, or type `@configure-mcp`. It asks which tool you want to connect,
whether the config is global or for this workspace, the transport and its
settings, and any secret references.

## Trust

Your user file starts automatically — you wrote it. The **project** file does not
start until you trust the current contents: Robi stores a SHA-256 of the file on
the workspace, and a missing file, a changed file, or a fresh workspace leaves
project servers off. Changing the file clears the match.

The trust control is designed but not yet wired in the shell: the config route
exposes whether the project bytes match the stored hash, but there is no Enable
button in the UI yet. Until then, treat project MCP servers as needing the
backend path.

## Transports

- **stdio** — Robi spawns the server as a child process. The child gets a
  scrubbed environment (no `DYLD_*`, `LD_PRELOAD`, `LD_LIBRARY_PATH`, or any name
  containing `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, or `CREDENTIAL`) and the
  workspace root as its working directory. `command` resolves on your login
  shell's `PATH` (plus the `path_entries` setting), so `uvx` and `npm` work even
  when Robi is started from Finder. It is **not** run inside the shell sandbox;
  connecting to a server is itself a trust decision.
- **streamable HTTP** — Robi connects to the URL. Redirects are followed only
  while the next host still matches the URL rule. There is no OAuth flow; a 401
  fails the server. Static headers authenticate.

One connection per enabled server per workspace is shared across that workspace's
sessions. The servers start when the workspace is the one open in the shell, and
they close when you switch to another workspace or remove this one. A server
that drops restarts with backoff (1s, 2s, 4s, 8s, then 30s), at most five times
in ten minutes.

## Tools and approval

Each remote tool is registered as `mcp_<server>_<tool>`, with client-side
characters outside `[A-Za-z0-9_-]` replaced by `_`. Built-in tools register
first, so a name collision with a built-in leaves the built-in in place. A server
may contribute up to 64 tools; a tool whose schema exceeds 16 KiB is skipped.

Every MCP tool asks on its **first** call in a session. Approving that call runs
it; a later call with the same server and tool runs without asking. The list of
approved pairs lives on the chat session and does not outlive it. A server's
`readOnlyHint` and `destructiveHint` are stored for the approval bar but never
change the decision.

## Logs

Each server writes its own log under `~/.robi/logs/mcp/<server_id>/`, one file
per connection. A line is an MCP message in either direction, one line of the
server's stderr, or a lifecycle note; header and env values are never written.
The file is mode `0600`, and the same seven-day prune as the process logs
applies. This is where to look when a server connects but misbehaves. The
**Settings → MCP** page lists each server and the exact directory for its log.

## Status

`GET /api/v1/workspaces/{id}/mcp` reports each configured server's status —
`disconnected` until the workspace is open, then `starting`, `connected`, or
`failed` — along with its tool count and icon. Icons are cached in
`~/.robi/mcp-icons.json`.

## Where this is specified

The host, the config files and trust hash, the transports, name sanitizing, and
the approval rules are in [MCP](../design/reach/mcp.md).
