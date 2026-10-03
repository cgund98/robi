# Configuration

Robi reads two files from its home directory, `~/.robi/`:

- `config.toml` — ordinary settings. Created on first read.
- `secrets.toml` — credentials. Must be mode `0600`; a file that group or world
  can read is refused.

Both are plain TOML. The key is a string and the value is a string, for example:

```toml
# ~/.robi/config.toml
model = "glm-5.3"
reasoning_effort = "medium"
lsp = "on"
```

```toml
# ~/.robi/secrets.toml
opencode_go_api_key = "sk-..."
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
| `opencode_go_api_key` | yes | none | Bearer credential for the provider. |
| `brave_search_api_key` | yes | none | Brave Search token for `web_search`. |
| `model` | no | `glm-5.3` | Default model id. |
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

Any key not on this list is refused. MCP reads `~/.robi/secrets.toml` for a
value named by a server's `{"secret": "<name>"}` field; that name is not part of
this whitelist.

## Environment variables

Set by the user or the shell, not by a settings file.

| Variable | Default | Effect |
|---|---|---|
| `ROBI_DATABASE_URL` | `sqlite://robi.db?mode=rwc` for `robi-api`; the desktop app uses `robi.db` in the app data directory | The session database. |
| `ROBI_BIND` | `127.0.0.1:1431`, then the next free port through `1450` | Listen address. Must be loopback when set. When unset, a taken `1431` falls through to the next port. When set, that address is used exactly. |
| `ROBI_EXTERNAL_API` | unset | Set to `1` with `pnpm tauri dev` to skip the in-process API and use `make dev-api` through the Vite proxy. Ignored in a packaged build. |
| `HOME` | — | Resolves the `~/.robi` directory. Required. |

`robi-api` and the desktop process honor `RUST_LOG` for tracing output
(default `robi=info`). Each start writes a new file under `~/.robi/logs/` named
`robi-api-<timestamp>.log`. An existing file is never truncated. Debug builds
also print the same events to stderr.

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
