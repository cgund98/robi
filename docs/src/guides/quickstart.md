# Quickstart

Get Robi running and send your first message. Robi is a desktop app: a Tauri
window over a local API, `robi-api`. For development the two run separately.

## Prerequisites

- Rust (a recent stable toolchain) and Cargo.
- Node.js and [pnpm](https://pnpm.io/).
- A provider credential. Robi speaks the OpenAI-compatible chat-completions wire
  format; the default endpoint is OpenCode Go, so set `opencode_go_api_key`.

## 1. Start the local API

The API owns the database, the providers, and the agent runtime.

```sh
make api
```

This runs `cargo run -p robi --bin robi-api` and listens on `127.0.0.1:1431`.
It creates the session database on first run.

## 2. Open the app

Run the desktop window and the web UI, against the API from step 1:

```sh
pnpm tauri dev
```

The Vite dev server listens on port **1430** and proxies `/api` to the API on
`1431`. `pnpm dev` alone serves the web UI in a browser without the Tauri shell;
run the API alongside it either way.

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
