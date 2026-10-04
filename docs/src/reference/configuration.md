# Configuration

Robi reads two files from its home directory, `~/.robi/`:

- `config.toml` — ordinary settings. Created on first read.
- `secrets.toml` — credentials. Must be mode `0600`; a file that group or world
  can read is refused.

Both are plain TOML. The key is a string and the value is a string, for example:

```toml
# ~/.robi/config.toml
model = "ocg_glm-5.3"
reasoning_effort = "medium"
lsp = "on"
max_iterations = "50"
subagent_max_iterations = "50"
subagent_timeout_seconds = "120"
tool_timeout_seconds = "120"
path_allow_read = """
~/pnpm
"""
path_allow_write = """
~/pnpm
"""
path_entries = """
~/pnpm
"""
web_search_approval = "on"
web_fetch_approval = "on"
```

```toml
# ~/.robi/secrets.toml
opencode_go_api_key = "sk-..."
anthropic_api_key = "sk-ant-..."
brave_search_api_key = "..."
```

The same keys are readable and writable over HTTP at `/api/v1/settings/{key}`.
Secret values are never returned by that route.

## Settings keys

An empty required-shape value (`model_ask`, `reasoning_effort`) inherits from the
key it points at. The default is written to the file on the first read, so the
files and later reads always agree.

| Key | Secret | Default | Notes |
|---|---|---|---|
| `opencode_go_api_key` | yes | none | Bearer credential for OpenCode Go models (`ocg_` prefix). |
| `anthropic_api_key` | yes | none | `x-api-key` credential for Anthropic models (`ant_` prefix). |
| `brave_search_api_key` | yes | none | Brave Search token for `web_search`. |
| `model` | no | `ocg_glm-5.3` | Default model id. The `ocg_`/`ant_` prefix selects the provider. |
| `model_ask` | no | none | Model for ask mode. Empty inherits `model`. |
| `model_plan` | no | none | Model for plan mode. Empty inherits `model`. |
| `model_agent` | no | none | Model for agent mode. Empty inherits `model`. |
| `reasoning_effort` | no | none | `low`, `medium`, or `high`. Unset uses the provider default. |
| `reasoning_effort_ask` | no | none | Effort for ask mode. Empty inherits `reasoning_effort`. |
| `reasoning_effort_plan` | no | none | Effort for plan mode. Empty inherits `reasoning_effort`. |
| `reasoning_effort_agent` | no | none | Effort for agent mode. Empty inherits `reasoning_effort`. |
| `base_url` | no | none | Provider base URL. Unset uses the provider default. |
| `system_prompt` | no | none | Extra system-prompt text. Unset adds no block. |
| `lsp` | no | `on` | `on` registers the language-server tools; `off` leaves them out. |
| `max_iterations` | no | `50` | Model turns in one primary-agent run. A whole number from 1 to 500. Read when the session actor starts. |
| `subagent_max_iterations` | no | `50` | Model turns in one explore or general child. A whole number from 1 to 500. Read when `delegate` starts that child. |
| `subagent_timeout_seconds` | no | `120` | Seconds before an explore or general child is stopped. A whole number from 1 to 3600. Read when `delegate` starts that child. |
| `tool_timeout_seconds` | no | `120` | Seconds before a `shell` command is killed. A whole number from 1 to 3600. Read when that command starts. |
| `path_allow_read` | no | none | Newline-separated paths appended to every session's read allow list. A line starting with `~/` is that user's home directory. Read from the store each time a tool builds the path filter or the shell profile. |
| `path_allow_write` | no | none | Newline-separated paths appended to every session's write allow list. A line starting with `~/` is that user's home directory. Read from the store each time a tool builds the path filter or the shell profile. |
| `path_entries` | no | none | Newline-separated directories appended to the sandbox `PATH`. Each directory is also appended to the read allow list. A line starting with `~/` is that user's home directory. Read from the store each time a shell builds its environment. |
| `web_search_approval` | no | `on` | `on` asks before every `web_search`. `off` runs the search without a card. Read when the session actor starts. |
| `web_fetch_approval` | no | `on` | `on` asks the first time a host is fetched in the session. `off` fetches without a card. Read when the session actor starts. |

Any key not on this list is refused. MCP reads `~/.robi/secrets.toml` for a
value named by a server's `{"secret": "<name>"}` field; that name is not part of
this whitelist.

## Environment variables

Set by the user or the shell, not by a settings file.

| Variable | Default | Effect |
|---|---|---|
| `ROBI_DATABASE_URL` | `sqlite://robi.db?mode=rwc` for `robi-api`. The packaged app uses `robi.db` in the app data directory. `pnpm tauri dev` uses `dev/robi.db` there | The session database. |
| `ROBI_BIND` | `127.0.0.1:1431`, then the next free port through `1450` | Listen address. Must be loopback when set. When unset, a taken `1431` falls through to the next port. When set, that address is used exactly. |
| `ROBI_EXTERNAL_API` | unset | Set to `1` with `pnpm tauri dev` to skip the in-process API and use `make dev-api` through the Vite proxy. Ignored in a packaged build. |
| `HOME` | — | Resolves the `~/.robi` directory. Required. |

`robi-api` and the desktop process honor `RUST_LOG` for tracing output
(default `robi=info`). Each start writes a new file under `~/.robi/logs/` named
`robi-api-<timestamp>.log`. A dev build, including `pnpm tauri dev` and
`robi-api` built for debug, uses `robi-dev-<timestamp>.log` instead, and each
prefix is pruned on its own. An existing file is never truncated. Debug builds
also print the same events to stderr. A failure starting the in-process API, and
any panic, is appended to that file and flushed before the process exits. The
background logger does not flush when the process aborts.

On startup the process keeps the new file and the newest older log, then
deletes the rest when a file is older than 7 days or when more than 8 files
would remain. The newest older log is kept either way, so the previous run is
still on disk after one reopen. A later start is what drops it. Secret values
are not written to these files.

The standalone example (`cargo run -p robi --example simple`) reads `ROBI_MODEL`,
`ROBI_BASE_URL`, `ROBI_EFFORT`, and `OPENCODE_GO_API_KEY`; those are example-only
and are not read by the API.

## Where this is specified

The store choice, the file modes, and the settings port are in
[Persistence](../design/persistence/persistence.md). The provider keys and the
base URL are in
[Providers and streaming](../design/providers/providers-streaming.md).
