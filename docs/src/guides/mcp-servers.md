# Setting up MCP servers

The **Model Context Protocol** (MCP) lets Robi call tools that other programs
provide. Robi is an MCP **host** (client): it connects to servers you configure,
and each remote tool joins the assistant's registry as
`mcp_<server>_<tool>`. Robi does not accept MCP connections from anyone.

MCP tools are registered in **agent** mode only. Ask and plan keep their built-in
tool sets, and subagents never receive MCP tools.

This guide is the walkthrough. For how the host behaves under the hood, see
[MCP](../concepts/mcp.md).

If you would rather be walked through it, load the bundled `configure-mcp`
skill (`/configure-mcp`, or just ask to add an MCP server). It asks where the
config belongs — global or this workspace — which tool you want to connect, the
transport and its settings, and any secret references, then writes the entry
and tells you to restart the app.

## 1. Choose an entry point to edit

Two JSON files share one shape:

- `~/.robi/mcp.json` — your servers, available in every workspace.
- `<workspace>/.robi/mcp.json` — servers for one project.

Either edit the file directly, or open **Settings → MCP**. That page lists the
servers Robi knows about and their connection state (the same list as the tray
in the top bar), shows both files as stored, and points at where the server logs
live. It has a **Refresh** button to reread the files and the list. A missing
file is an empty server list.

The config is read when the workspace becomes the open one, and again on an
agent-mode turn for a server that is not already connected. An edit therefore
applies the next time the workspace is focused, or on the next message if the
server is not up yet.

## 2. Write a server entry

A server has a `command` (stdio) **or** a `url` (HTTP), not both.

**stdio** — Robi spawns the server as a child process:

```json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": { "secret": "github_token" }
      }
    }
  }
}
```

**HTTP** — Robi connects to the URL:

```json
{
  "mcpServers": {
    "linear": {
      "url": "https://mcp.linear.app/mcp",
      "headers": {
        "Authorization": { "secret": "linear_token" }
      }
    }
  }
}
```

Fields that are enforced:

| Field | Applies to | Rule |
|---|---|---|
| server id | both | matches `^[A-Za-z0-9_-]{1,32}$`; any other key is skipped and logged |
| `command` | stdio | an executable name, or an absolute path; a relative path is a config error |
| `args` | stdio | array of strings; default `[]` |
| `env` | stdio | map overlaid on the scrubbed environment |
| `url` | HTTP | `https`, or `http` on `127.0.0.1` or `localhost` |
| `headers` | HTTP | map of header values |
| `timeout_seconds` | both | whole-call budget; default 60, above 300 is a config error |
| `enabled` | both | `false` skips the server |

Unknown fields are ignored, so a config copied from another host still parses.

## 3. Reference secrets

An `env` or `headers` value is either a plain string or `{ "secret": "<name>" }`.
A plain string is passed through. A secret object reads `<name>` from
`~/.robi/secrets.toml` and is never written back — so a token stays out of the
JSON you might commit. A missing secret skips that server and logs it; resolved
values are never logged.

## 4. Enable and trust

Your user file starts automatically — you wrote it.

The **project** file (`<workspace>/.robi/mcp.json`) does not start until you trust
its current contents. Robi stores a SHA-256 of the file on the workspace, and a
missing file, a changed file, or a fresh workspace leaves project servers off;
changing the file clears the match so a newly committed server does not start on
the next message.

> The trust control is designed but not yet wired into the shell: the config
> route reports whether the project bytes match the stored hash, but there is no
> **Enable** button in the UI yet. Until then, treat project MCP servers as
> needing the backend path.

The same id in both files: the project entry replaces the user entry, and only
after the project file is trusted. Until then the user entry stays.

## 5. Start and see status

One connection per enabled server per workspace is shared by that workspace's
sessions. The shell starts them when the workspace becomes the open one, and an
agent-mode turn starts any that are not up yet. Switching workspaces, or deleting
one, closes its servers. A server that drops restarts with backoff (1s, 2s, 4s,
8s, then 30s), at most five times in ten minutes; after that it is `failed` until
you retry or change the config.

The sidebar **MCP** tray shows a mark per configured server, coloured by status —
`connected`, `starting`, `failed`, or `disconnected`. Hovering a mark shows the
name, status, and (when connected) its tool count, and clicking it opens
**Settings → MCP**.

**stdio** servers run with a scrubbed environment — no `DYLD_*`, `LD_PRELOAD`,
`LD_LIBRARY_PATH`, or any name containing `KEY`, `TOKEN`, `SECRET`, `PASSWORD`,
or `CREDENTIAL` — with the workspace root as the working directory. They are
**not** run inside the shell sandbox: connecting to a server is itself the trust
decision, and it needs the network and the credentials its config names.
`command` resolves on your login shell's `PATH` (plus the `path_entries`
setting), so `uvx` and `npm` work even when Robi is launched from Finder.

**HTTP** redirects are followed only while the next host still matches the `url`
rule. There is no OAuth flow; a `401` fails the server. Static headers
authenticate.

## 6. Approve calls

Every MCP tool asks on its **first** call in a session. The approval line is
**Call** and `server / tool`, with the pretty-printed arguments beneath it. When
the server set a hint, a muted line adds **Server says: read-only**,
**Server says: destructive**, or **Server says: reaches the network** — these are
the server's labels and never change the decision.

- **Approve** runs this call.
- **Allow for this session** runs it and remembers that server + tool pair, so
  later calls run without asking.
- **Reject** does not call the server.

The remembered list lives on the chat session and does not outlive it. A server's
`readOnlyHint` and `destructiveHint` are stored for the bar but never skip
approval.

## Logs

Each server writes its own log under `~/.robi/logs/mcp/<server_id>/`, one file
per connection. It captures the MCP messages in both directions and the server's
own stderr, so this is where to look when a server connects but a tool
misbehaves. Header and env values are never written. Older files are pruned on
the same seven-day schedule as the process logs.

## Limits

- A server may contribute up to 64 tools; the rest are logged and omitted.
- A tool whose `inputSchema` JSON exceeds 16 KiB is skipped.
- A name that collides with a built-in leaves the built-in in place.
- A large result is compressed before it reaches the model; the original is
  retrievable. See [MCP output](../design/compression/mcp-output.md).

## Where this is specified

The host, the config files and trust hash, the transports, name sanitizing, and
the approval rules are in [MCP](../design/reach/mcp.md). The compression path is
in [MCP output](../design/compression/mcp-output.md).

## Next

- [MCP](../concepts/mcp.md) — how the host behaves.
- [Subagents](../concepts/subagents.md) — why children never get MCP tools.
- [Approvals](../concepts/approvals.md) — how the approval gates compose.
