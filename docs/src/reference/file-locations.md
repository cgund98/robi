# File locations

Robi keeps everything under one home directory, `~/.robi`, unless an environment
variable says otherwise. The directory is created mode `0700` on first use.

| Path | What it holds |
|---|---|
| `~/.robi/` | The home directory. |
| `~/.robi/config.toml` | Ordinary settings. See [Configuration](configuration.md). |
| `~/.robi/secrets.toml` | Credentials. Mode `0600`, refused when readable by group or world. |
| `~/.robi/plans/<session_id>/` | Legacy plan files from before S9. New plans are written to `<workspace>/.robi/plans/` and these are no longer read. |
| `~/.robi/sessions/<session_id>/blobs.redb` | Tool results, tool originals, and image bytes for one chat. It is also that chat's image index: an image id resolves only inside its own session file. |
| `~/.robi/locks/sessions/<session_id>.lock` | Held while a process owns a chat. Contains the holder's pid and scope. |
| `~/.robi/locks/paths/<sha256>.lock` | One workspace file, held across a read-modify-write. Removed after seven days if nothing holds it. |
| `~/.robi/locks/startup.lock` | Held while a process runs migrations, the blob evacuation, the prune, and the turn-display repair. |
| `~/.robi/locks/settings.lock` | Held across a `config.toml` or `secrets.toml` write. |
| `~/.robi/index/<workspace_id>/` | A legacy semantic index from a build before M11. Nothing writes it now; deleting the workspace still removes the directory. |
| `~/.robi/mcp.json` | User-scoped MCP server config. |
| `~/.robi/mcp-icons.json` | Cached MCP server icons (capped at 256 KiB). |
| `~/.robi/logs/` | Process logs. One file per start, `robi-api-<timestamp>.log` or `robi-dev-<timestamp>.log`; the newest prior file is kept, older than seven days is pruned. |
| `~/.robi/logs/mcp/<server_id>/` | One MCP server's logs, one `mcp-<timestamp>.log` per connection. Mode `0600`. See [MCP](../concepts/mcp.md). |

The lock files are empty apart from one line naming the holder: `pid 4812 scope
turn database sqlite://… at …`. A refused process reads that line, so the message
it shows names the pid that has the session. There is no TTL and no heartbeat:
the operating system drops a file lock when the holding process exits, including
on `kill -9`, so a stale lock cannot outlive its owner.

## The session database

The session store is SQLite. `robi-api` uses the path named by
`ROBI_DATABASE_URL`, which defaults to `sqlite://robi.db?mode=rwc` — a `robi.db`
file relative to the working directory you started `robi-api` in, not under
`~/.robi`. The desktop app, when that variable is unset, stores `robi.db` in
the app data directory (`~/Library/Application Support/com.github.cgund98.robi`
on macOS). `pnpm tauri dev` uses `dev/robi.db` in that same directory, so a
migration applied while developing does not change the database the installed
app opens. WAL mode adds the `robi.db-wal` and `robi.db-shm` sidecar files, both
gitignored.

**The terminal app chooses differently.** With `ROBI_DATABASE_URL` unset,
`robi` opens the desktop app's own database —
`~/Library/Application Support/com.github.cgund98.robi/robi.db` on macOS — rather
than a file relative to the working directory. A debug build, which is what a
`cargo run` from a checkout is, opens the app's **dev** database instead
(`.../com.github.cgund98.robi/dev/robi.db`), so it shows the same workspaces
`pnpm tauri dev` shows, and a migration applied while developing does not reach
the installed app's file. It also opens the workspace named by its first
argument, or the working directory when there is none, and asks before creating a
workspace at a path that has none. See
[the terminal shell](../design/shell/tui.md#process-model) and
[the workspace argument](../design/shell/tui.md#the-workspace-argument).

Point it at a fixed location if you want the database independent of the shell's
working directory:

```sh
ROBI_DATABASE_URL=sqlite:///Users/you/.robi/sessions.db?mode=rwc make api
```

## Workspace files

A workspace is a directory you opened in Robi. Three things may be written into
it:

- `<workspace>/.robi/mcp.json` — a project-scoped MCP server config. It does not
  start until you trust the current file hash. See [MCP](../concepts/mcp.md).
- `<workspace>/.robi/skills/` — project skills, found by the skill scan. See
  [Skills](../concepts/skills.md).
- `<workspace>/.robi/plans/<name>/PLAN.md` and `TODO.md` — a plan and its
  checklist, written by the agent in Plan or Agent mode. `.robi/plans/.gitignore`
  holds a single `*`, so the directory ignores itself and nothing in it is
  tracked. See [Tools](tools.md#plans).

## Where this is specified

The schema and the data directory are in
[Persistence](../design/persistence/persistence.md). The MCP config files are in
[MCP](../design/reach/mcp.md).
