# Configuration

Robi reads two files from its home directory, `~/.robi/`:

- `config.toml` — ordinary settings. Created on first read.
- `secrets.toml` — credentials. Must be mode `0600`; a file that group or world
  can read is refused.

The app reads and writes both files for you through **Settings**; editing them
by hand is only needed for headless or manual setups. See
[Quickstart](../guides/quickstart.md) for the in-app flow.

Both are plain TOML. The key is a string and the value is a string, for example:

```toml
# ~/.robi/config.toml
model = "ocg_glm-5.3"
reasoning_effort = "medium"
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
file_open_patterns = """
*.png
"""
file_open_command = "zed {file_path}:{line}"
web_search_approval = "on"
web_fetch_approval = "on"
sandbox_mechanism = "os"
sandbox_image = "robi/agent:0.1.0"
sandbox_runtime = "runsc"
sandbox_memory_mb = "4096"
sandbox_cpus = "2"
sandbox_pids_limit = "512"
sandbox_network = "on"
sandbox_project_files = "off"
```

```toml
# ~/.robi/secrets.toml
opencode_go_api_key = "sk-..."
anthropic_api_key = "sk-ant-..."
deepseek_api_key = "sk-..."
brave_search_api_key = "..."
```

The same keys are readable and writable over HTTP at `/api/v1/settings/{key}`,
and from the terminal app's settings surface (`Ctrl-,` or `/settings`). The terminal
shows the GUI's six pages — General, Model providers, Permissions, Sandbox, MCP,
Audit log — and writes one key per `Enter`. A secret is never read back: the
surface shows `••••••••` once one is stored, and an empty field leaves it alone.
An empty field on any other key removes it, so the default takes over again.
`GET /api/v1/settings?key=` reads several keys in one request; repeat `key`
for each one. Secret values are never returned by either route.

## Settings keys

An empty required-shape value (`model_ask`, `reasoning_effort`) inherits from the
key it points at. The default is written to the file on the first read, so the
files and later reads always agree.

| Key | Secret | Default | Notes |
|---|---|---|---|
| `opencode_go_api_key` | yes | none | Bearer credential for OpenCode Go models (`ocg_` prefix). |
| `anthropic_api_key` | yes | none | `x-api-key` credential for Anthropic models (`ant_` prefix). |
| `deepseek_api_key` | yes | none | Bearer credential for DeepSeek models (`dsk_` prefix). |
| `provider_opencode_go` | no | `on` | `off` hides OpenCode Go models from the picker and refuses a turn that needs one. |
| `provider_anthropic` | no | `on` | `off` hides Anthropic models from the picker and refuses a turn that needs one. |
| `provider_deepseek` | no | `on` | `off` hides DeepSeek models from the picker and refuses a turn that needs one. |
| `brave_search_api_key` | yes | none | Brave Search token for `web_search`. |
| `model` | no | `ocg_glm-5.3` | Default model id. The `ocg_`/`ant_`/`dsk_` prefix selects the provider. |
| `model_ask` | no | none | Model for ask mode. Empty inherits `model`. |
| `model_plan` | no | none | Model for plan mode. Empty inherits `model`. |
| `model_agent` | no | none | Model for agent mode. Empty inherits `model`. |
| `reasoning_effort` | no | none | `low`, `medium`, or `high`. Unset uses the provider default. |
| `reasoning_effort_ask` | no | none | Effort for ask mode. Empty inherits `reasoning_effort`. |
| `reasoning_effort_plan` | no | none | Effort for plan mode. Empty inherits `reasoning_effort`. |
| `reasoning_effort_agent` | no | none | Effort for agent mode. Empty inherits `reasoning_effort`. |
| `base_url` | no | none | Base URL for OpenCode Go and Anthropic. Unset uses that provider's default. |
| `deepseek_base_url` | no | none | Base URL for DeepSeek. Unset uses `https://api.deepseek.com`. |
| `system_prompt` | no | none | Extra system-prompt text. Unset adds no block. |
| `max_iterations` | no | `50` | Model turns in one primary-agent run. A whole number from 1 to 500. Read when the session actor starts. |
| `subagent_max_iterations` | no | `50` | Model turns in one explore or general child. A whole number from 1 to 500. Read when `delegate` starts that child. |
| `subagent_timeout_seconds` | no | `120` | Seconds before an explore or general child is stopped. A whole number from 1 to 3600. Read when `delegate` starts that child. |
| `tool_timeout_seconds` | no | `120` | Seconds before a `shell` command is killed. A whole number from 1 to 3600. Read when that command starts. |
| `path_allow_read` | no | none | Newline-separated paths appended to every session's read allow list. A line starting with `~/` is that user's home directory. Read from the store each time a tool builds the path filter or the shell profile. |
| `path_allow_write` | no | none | Newline-separated paths appended to every session's write allow list. A line starting with `~/` is that user's home directory. Read from the store each time a tool builds the path filter or the shell profile. |
| `path_entries` | no | none | Newline-separated directories appended to the sandbox `PATH` and to an MCP stdio child's `PATH`. Each directory is also appended to the read allow list. A line starting with `~/` is that user's home directory. Read from the store each time a shell or an MCP stdio child builds its environment. |
| `file_open_patterns` | no | none | Newline-separated globs. `*` matches any run of characters, including `/`, and `?` matches one character. A workspace-relative path that matches one opens outside the file viewer when its name is clicked on an edit card. Anything else opens in the file viewer. Absent matches nothing. |
| `file_open_command` | no | none | One line. `{file_path}` is the absolute path and `{line}` is the 1-based line of the first change. A click with no line drops `{line}` and a colon before it. The line is split on whitespace into arguments; double quotes group one argument. It is not a shell command. Absent uses the system app. The desktop app posts the path to `POST /api/v1/workspaces/{id}/open` and waits up to 10 seconds. A program still running then has started. A failure to start, or a non-zero exit inside that window, is shown on the edit card. |
| `web_search_approval` | no | `on` | `on` asks before every `web_search`. `off` runs the search without a card. Read when the session actor starts. |
| `web_fetch_approval` | no | `on` | `on` asks the first time a host is fetched in the session. `off` fetches without a card. Read when the session actor starts. |
| `sandbox_mechanism` | no | `os` | `os` runs commands under Seatbelt on macOS or bubblewrap on Linux. `container` runs them in a gVisor container over the Docker API. Read before every `shell` command. |
| `sandbox_image` | no | `robi/agent:0.1.0` | Image the container starts from, as `repo/name[:tag]`. Whitespace and a leading `-` are refused. Read when the container is created and re-resolved on every command. |
| `sandbox_runtime` | no | `runsc` | A runtime name in the daemon's runtime map, up to 64 characters of `[A-Za-z0-9_.-]`. |
| `sandbox_memory_mb` | no | `4096` | Container memory limit in MiB, a whole number from 512 to 65536. |
| `sandbox_cpus` | no | `2` | Container CPU limit in whole CPUs, from 1 to the machine's CPU count. |
| `sandbox_pids_limit` | no | `512` | Most processes the container may run at once, a whole number from 64 to 8192. |
| `sandbox_network` | no | `on` | `on` keeps the container's network sandboxed: no interface unless a single command is approved for egress, or the workspace joins a service network. `off` gives every sandboxed command full outbound networking through the daemon's NAT, and no approval card appears for egress because there is nothing left to grant. Global only: a workspace file cannot change it. |
| `sandbox_project_files` | no | `off` | `on` lets a workspace's `.robi/sandbox.toml` narrow the sandbox values above, and name services and `[env]` entries that have no global key. `off` ignores the file. A project file can never choose `sandbox_mechanism`. |

The container keys are read only when `sandbox_mechanism` is `container`; they are
otherwise stored and unused. The **Settings → Sandbox** screen writes all eight.

Any key not on this list is refused. `lsp` is gone with the language-server
tools (S4): writing it returns `unknown setting: lsp`. MCP reads
`~/.robi/secrets.toml` for a value named by a server's `{"secret": "<name>"}`
field; that name is not part of this whitelist.

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

MCP servers log separately, one file per connection under
`~/.robi/logs/mcp/<server_id>/` and mode `0600`. Each line is a JSON-RPC message
in either direction, one line of the child's stderr, or a lifecycle note. The
same seven-day and eight-file policy applies, and a server taken out of the
config has its directory pruned on the next start. Header and env values are
never written. See [MCP](../concepts/mcp.md).

An MCP stdio child resolves its `command` on the user's login-shell `PATH`
(`$SHELL -ilc`, falling back to this process's `PATH`) with `path_entries`
appended. That is what lets `uvx` and `npm` resolve when Robi is launched from
Finder, where the process `PATH` is the minimal system one.

The standalone example (`cargo run -p robi --example simple`) reads `ROBI_MODEL`,
`ROBI_BASE_URL`, `ROBI_EFFORT`, and `OPENCODE_GO_API_KEY`; those are example-only
and are not read by the API.

## Where this is specified

The store choice, the file modes, and the settings port are in
[Persistence](../design/persistence/persistence.md). The provider keys and the
base URL are in
[Providers and streaming](../design/providers/providers-streaming.md).
