---
name: Configure MCP
description: Guided setup for an MCP server. Load this when the user wants to add, configure, or set up an MCP server.
---

Guide the user through adding one MCP server. Ask the questions one at a time and do not write the file until they confirm the draft.

1. Ask which tool or service they want to connect. This names the server id and decides the transport: a local tool Robi launches uses `command` (stdio), a hosted service uses `url` (HTTP), and a server has exactly one of the two.
2. Ask where the config belongs.
   - Global, `~/.robi/mcp.json`, applies to every workspace. This is the default.
   - This workspace, `<workspace>/.robi/mcp.json`, applies to this project only.
   Both files share one shape rooted at `"mcpServers"`, and a project entry with the same id replaces the global one.
3. Ask for the transport settings.
   - **stdio:** `command` as an executable name or an absolute path (a relative path is a config error), `args` as an array of strings, and any `env` the server needs.
   - **HTTP:** `url`, which must be `https` or `http` on `127.0.0.1` or `localhost`, and any `headers`.
4. Ask about secrets. A token or key belongs in `~/.robi/secrets.toml`, not in the JSON, so it stays out of a file the user might commit. Write the config value as `{ "secret": "<name>" }` and make sure `<name>` matches a secret entry in `secrets.toml`. A plain string is passed through, which is how an existing config pastes in. A missing secret skips that server and is logged; resolved values are never logged.
5. Ask whether to set `timeout_seconds` (default 60; above 300 is a config error) and whether to set `enabled: false` to park the server.
6. Show the JSON entry and the file path for confirmation, then write it. If the file already has an `"mcpServers"` object, add the new id to it instead of replacing the file.
7. Tell the user to restart the app for the change to take effect. The config is read when a workspace is focused and again on an agent-mode turn, so a restart is the reliable way to pick it up.

The server id matches `^[A-Za-z0-9_-]{1,32}$`. A project server also has to be trusted: the project file does not start until the workspace has stored the SHA-256 of its current contents, and editing the file clears that match. Note that the trust control is not yet wired into the shell, so a project server may need the backend path until an Enable button ships.

The reply names the file path and the server id, and points the user at the MCP servers guide (`docs/src/guides/mcp-servers.md`) for the full walkthrough.
