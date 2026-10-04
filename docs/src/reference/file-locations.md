# File locations

Robi keeps everything under one home directory, `~/.robi`, unless an environment
variable says otherwise. The directory is created mode `0700` on first use.

| Path | What it holds |
|---|---|
| `~/.robi/` | The home directory. |
| `~/.robi/config.toml` | Ordinary settings. See [Configuration](configuration.md). |
| `~/.robi/secrets.toml` | Credentials. Mode `0600`, refused when readable by group or world. |
| `~/.robi/plans/<session_id>/` | Plan files written by the assistant, one directory per session. |
| `~/.robi/sessions/<session_id>/blobs.redb` | Tool results, tool originals, and image bytes for one chat. |
| `~/.robi/sessions/image-index.redb` | Image id to session id. No image bytes. |
| `~/.robi/index/<workspace_id>/index.sqlite` | The semantic index for one workspace. |
| `~/.robi/models/` | Cached embedding model weights. |
| `~/.robi/mcp.json` | User-scoped MCP server config. |
| `~/.robi/mcp-icons.json` | Cached MCP server icons (capped at 256 KiB). |

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

Point it at a fixed location if you want the database independent of the shell's
working directory:

```sh
ROBI_DATABASE_URL=sqlite:///Users/you/.robi/sessions.db?mode=rwc make api
```

## Workspace files

A workspace is a directory you opened in Robi. Nothing is written into it by
default. Two exceptions:

- `<workspace>/.robi/mcp.json` — a project-scoped MCP server config. It does not
  start until you trust the current file hash. See [MCP](../concepts/mcp.md).
- `<workspace>/.robi/skills/` — project skills, found by the skill scan. See
  [Skills](../concepts/skills.md).

## Where this is specified

The schema and the data directory are in
[Persistence](../design/persistence/persistence.md). The index file and the
model cache are in
[Semantic search](../design/intelligence/semantic-search.md). The MCP config
files are in [MCP](../design/reach/mcp.md).
