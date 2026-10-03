# Quickstart

Get Robi running and send your first message. Robi is a desktop app: a Tauri
window that starts the local API in the same process. `robi-api` is still the
process you run for a browser-only UI, and for headless use.

## Prerequisites

- Rust (a recent stable toolchain) and Cargo.
- Node.js and [pnpm](https://pnpm.io/).
- A provider credential. Robi speaks the OpenAI-compatible chat-completions wire
  format; the default endpoint is OpenCode Go, so set `opencode_go_api_key`.

## 1. Open the app

The desktop window starts the API itself. The database is created on first run
under the app data directory.

```sh
pnpm tauri dev
```

The Vite dev server listens on port **1430**. The API binds `127.0.0.1:1431`,
or the next free port when that one is taken. The window reads the bound origin
before it talks to the API.

To keep using `make dev-api` (so the API restarts without rebuilding the window),
skip the in-process server:

```sh
pnpm tauri:external-api
```

That sets `ROBI_EXTERNAL_API=1`. The webview then uses the Vite `/api` proxy to
`127.0.0.1:1431`, and you run `make dev-api` beside it. A packaged build ignores
the flag.

`pnpm dev` alone serves the web UI in a browser. It does not start the API, so
run `make api` alongside it. That listens on `127.0.0.1:1431`, and Vite proxies
`/api` there.

## 3. Add a credential

Put the provider key in `~/.robi/secrets.toml`:

```toml
opencode_go_api_key = "sk-..."
```

The file must be mode `0600`. The app reads it at startup; see
[Configuration](../reference/configuration.md) for every key. An optional
`brave_search_api_key` enables `web_search`.

## 4. Open a workspace and send a message

Open a folder as a workspace, start a session, and type into the composer. The
assistant reads files, searches, and proposes edits. In **agent** mode it can
write files and run commands, always inside the safety model:

- a read inside the workspace is free;
- an edit to a protected path, a shell command that leaves the default sandbox,
  or a web fetch to a new host pauses for your approval;
- the approval bar shows the exact arguments, not a summary.

See [Approvals](../concepts/approvals.md) for what does and does not ask.

## Common commands

| Task | Command |
|---|---|
| Local API | `make api` |
| Restart the API on Rust changes | `make dev-api` |
| Desktop app | `pnpm tauri dev` |
| Web-only Vite dev | `pnpm dev` (port 1430) |
| Test the Rust workspace | `make test` |
| Lint and format | `make lint`, `make fix` |
| Build these docs | `make docs`, `make docs-serve` |

## Next

- [How Robi works](../concepts/how-robi-works.md) — the layers and the loop.
- [Tools](../reference/tools.md) — what the assistant can call.
- [Configuration](../reference/configuration.md) — every setting and environment
  variable.
