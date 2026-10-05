# Quickstart

Get Robi running and send your first message. Setup happens entirely in the app:
open it, open a workspace, set up model providers, choose your default model,
then start working. Robi is a desktop app — a Tauri window that starts the local
API in the same process.

## Install the app

Download the latest build from the [releases page](https://github.com/cgund98/robi/releases) and open it.

### Run from source

To build it yourself instead, you need:

- Rust (a recent stable toolchain) and Cargo.
- Node.js and [pnpm](https://pnpm.io/).

```sh
pnpm tauri dev
```

The Vite dev server listens on port **1430**. The API binds `127.0.0.1:1431`, or
the next free port when that one is taken, and the window reads the bound origin
before it talks to the API.

To restart the API without rebuilding the window, skip the in-process server:

```sh
pnpm tauri:external-api   # sets ROBI_EXTERNAL_API=1, webview uses the Vite /api proxy
make dev-api              # run beside it
```

`pnpm dev` alone serves the web UI in a browser and does not start the API; run
`make api` alongside it. A packaged build ignores the flag.

## 1. Open the app

On first launch there are no workspaces, so Robi opens the **Workspaces** page.

## 2. Open a workspace

Click **New workspace** and pick a directory in the system folder dialog. The
name is taken from the folder. Sessions stay with that folder, and the agent
works from its root. You can add another at any time from the sidebar workspace
switcher (**Add workspace…**).

## 3. Set up model providers

Open **Settings → Model Providers** (the sidebar **Settings** button lands here).
Each provider has an **Enabled** switch and an **API key** field. Type a key and
it saves when the field loses focus; the status line reads **Saved**. The server
never returns the value, so a saved key shows as `••••••••` with the hint **A key
is saved. Enter a new value to replace it.**

| Provider | Model ids | Extra |
|---|---|---|
| OpenCode Go | `ocg_` | Optional **Base URL** |
| Anthropic | `ant_` | |
| DeepSeek | `dsk_` | Optional **Base URL** (default `https://api.deepseek.com`) |

An optional **Brave API key** under **Web search** enables `web_search`; each
search waits for approval.

Keys are written to `~/.robi/secrets.toml` (mode `0600`) and other settings to
`~/.robi/config.toml` — you never hand-edit these; the UI does it. See
[Configuration](../reference/configuration.md) for every key.

## 4. Set default model preferences

Open **Settings → General → Model Defaults**. Pick a **Global** model and
reasoning effort (**low** / **medium** / **high**), then optionally override the
model and effort for the **Agent**, **Ask**, and **Plan** modes. Any mode left
unset uses the Global model and effort.

## 5. Start working

Click **New chat**, then describe the task in the composer
(**Describe a task or ask a question**). Before sending you can pick a mode
(**Ask**, **Plan**, **Agent**) and override the model and effort for that session
only. A session is created on the first send.

The assistant reads files, searches, and proposes edits. In **agent** mode it
can write files and run commands, always inside the safety model:

- a read inside the workspace is free;
- an edit to a protected path, a shell command that leaves the default sandbox,
  or a web fetch to a new host pauses for your approval;
- the approval bar shows the exact arguments, not a summary.

See [Approvals](../concepts/approvals.md) for what does and does not ask.

## Common commands

| Task | Command |
|---|---|
| Desktop app | `pnpm tauri dev` |
| Local API | `make api` |
| Restart the API on Rust changes | `make dev-api` |
| Web-only Vite dev | `pnpm dev` (port 1430) |
| Test the Rust workspace | `make test` |
| Lint and format | `make lint`, `make fix` |
| Build these docs | `make docs`, `make docs-serve` |

## Next

- [How Robi works](../concepts/how-robi-works.md) — the layers and the loop.
- [Tools](../reference/tools.md) — what the assistant can call.
- [Configuration](../reference/configuration.md) — every setting and environment
  variable.
