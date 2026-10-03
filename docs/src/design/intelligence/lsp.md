# Language servers

This page settles **D8** and defines how a workspace's language servers
become tools the model can call. It is the design for **F7.1** in the
[roadmap](../../roadmap.md). Path rules are in [read-tools.md](../tools/read-tools.md).
The write tools that must notify a running server are in
[editing-tools.md](../tools/editing-tools.md). Modes are in
[agent-modes.md](../core/agent-modes.md).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Chunking, embeddings, and fusing symbol hits into `semantic_search` | [semantic-search.md](semantic-search.md). This page does not write the index |
| Running `cargo test` or another build | [shell-tool.md](../tools/shell-tool.md) |
| A buffer, a tab, or a diagnostics panel | Robi is not an editor. The tool result is the model's view, and the existing tool card is the user's |
| Completion, inlay hints, semantic tokens, and call hierarchy | Not tools. An editor paints those; the model does not |

## Problem

After an edit, the model has to know whether the file still typechecks.
`grep` finds a string. It cannot tell a definition from a comment, and it
cannot see a type error. A shell build can, and it is slow, noisy, and a
different command in every language.

The user often already has a language server installed for their editor.
That process knows the symbol graph and the compiler diagnostics. The
model should call it. A missing server must not fail the turn: the model
falls back to `grep` and `shell`, which already work.

## Decision

`async-lsp` speaks the protocol. `lsp-types` is the message shape. Both
live in `crates/robi`. `robi-core` does not spawn a process and does not
name a language. The client is a module, `crates/robi/src/lsp/`. The
tools that call it live in `crates/robi/src/tools/`.

### D8: `async-lsp` and `lsp-types`

A language server is a peer. It sends requests the client must answer,
or it stalls. `rust-analyzer` asks for `workspace/configuration` and
`workspace/workspaceFolders` during startup. A client that only writes
requests and reads responses deadlocks on the first real server.

`async-lsp` is a tokio client and server on top of `lsp-types`. It
frames stdio, matches request ids, and dispatches the server's requests
to handlers we implement. `tower-lsp` builds a language server.

The handlers we implement are small and fixed:

| Server request or notification | Answer |
|---|---|
| `workspace/workspaceFolders` | The one workspace root |
| `workspace/configuration` | The static settings in the catalog row, or an empty object |
| `window/workDoneProgress/create` | Accept, and remember the token until it ends |
| `client/registerCapability` | Accept. Ignore registrations for methods this page does not call |
| `workspace/applyEdit` | `applied: false`. The server does not write the disk |
| `window/showMessage`, `window/logMessage`, `$/progress` | Log. Do not put them in the tool result |
| `textDocument/publishDiagnostics` | Store by URI, tagged with the document version |

Anything else returns method-not-found. A server command
(`workspace/executeCommand`) is never run.

### Catalog

The supported set is a table in source. Adding a language is a row.
The model cannot pass an argv. v1 ships these rows:

| Server id | Language ids | Extensions | Argv |
|---|---|---|---|
| `rust-analyzer` | `rust` | `.rs` | `rust-analyzer` |
| `typescript-language-server` | `typescript`, `javascript` | `.ts` `.tsx` `.mts` `.cts` `.js` `.jsx` `.mjs` `.cjs` | `typescript-language-server --stdio` |
| `ruff` | `python` | `.py` `.pyi` | `ruff server` |
| `pyright` | `python` | `.py` `.pyi` | `pyright-langserver --stdio` |
| `gopls` | `go` | `.go` | `gopls` |
| `clangd` | `c`, `cpp` | `.c` `.h` `.cc` `.cpp` `.cxx` `.hh` `.hpp` | `clangd` |

One process serves every language id in its row. TypeScript and
JavaScript share a process. C and C++ share a process. Two rows may
share an extension. The client starts the first row in the table
whose binary is on `PATH`. Python prefers `ruff server`. It starts
`pyright-langserver` when `ruff` is absent. Ruff publishes lint
diagnostics. It does not answer definition, references, or workspace
symbols, so those tools return `available: false` while Ruff is the
process for that workspace.

Discovery looks up the argv's first token on `PATH`. There is no
download, no version-manager probe, and no read of another editor's
config. A missing binary is `available: false` with `reason: "no_server"`.
An extension with no row is `reason: "unsupported"`. Both are a
successful tool result. The turn continues.

### Host binaries

The `robi-api` process looks up each binary on the `PATH` it inherited
when it started. A directory added to the shell afterward is invisible
until the API is restarted. `node_modules/.bin` is not searched, so a
package installed as a project dependency is not found unless that
directory is already on the API's `PATH`.

| Binary | Install |
|---|---|
| `rust-analyzer` | `rustup component add rust-analyzer` |
| `typescript-language-server` | `npm install -g typescript-language-server` |
| `ruff` | The `ruff` package for the host. Preferred for Python when it is on `PATH` |
| `pyright-langserver` | `npm install -g pyright`. Used for Python when `ruff` is absent |
| `gopls` | `go install golang.org/x/tools/gopls@latest` |
| `clangd` | The LLVM package for the host |

One of `ruff` or `pyright-langserver` is enough for Python. Both
installed means `ruff`. TypeScript and JavaScript share
`typescript-language-server`. C and C++ share `clangd`. The global `lsp`
setting must be `on`, which is the default. `off` leaves the tools
unregistered, so a binary on `PATH` is not started.

`rust-analyzer` settings sent for `workspace/configuration` are
`checkOnSave: true`, `cargo.targetDir: true`, and `cargo.extraArgs:
["--locked"]`. The check is what makes a diagnostic a compiler error
rather than a parse error. The target directory is `target/rust-analyzer`,
and `--locked` is meant to keep `cargo metadata` and `cargo check` from
rewriting `Cargo.lock`. Those two settings do not stop the restart
described under [Failure modes](#failure-modes). The other rows send no
settings.

### Process

The hub is one supervisor in the `robi-api` process. The key is the
canonical workspace root plus the server id. Two sessions on that root
share one `rust-analyzer`. A different root gets its own process.

A server starts on the first tool call that needs it. It is not started
by a write. A successful start is logged at info with the server id and
the root. A start that fails is logged at error, and so is the second
failure that marks the server failed. An unexpected exit is logged at
warn. An idle stop is logged at info. A missing binary is logged at warn
once for that server. Each request logs success at info. A timeout or a
protocol error is a warn, and a server that stops mid-request is an
error. Opening, updating, and closing a document are logged at info.
`window/showMessage` and `window/logMessage` follow the server's
severity: error, warning, and info are logged at those levels, and other
lines stay at debug. stderr stays at debug while the server is
running. When startup fails, the first line of stderr is included
in that error and in the `server_failed` hint. Startup is
`initialize` with that root as the only workspace folder, then
`initialized`. The root is the session workspace. The
client does not walk above it to find a Cargo workspace or a
`go.mod`. The server may walk, inside its own process.

The child inherits the API process environment, then drops the same
secret names the shell drops. It is not inside the shell sandbox.
`rust-analyzer` has to run `cargo`, and `gopls` has to run the go
command. The argv is ours, not the model's. stderr is logged. The
tool result includes it only when startup fails.

Idle for five minutes with no request and no open progress token
sends `shutdown`, then `exit`, then the process is killed if it is
still alive. A crash restarts once. A second failure marks that server
`server_failed` for the life of the API process, and later calls
return `available: false` without spawning. Restarting `robi-api`
clears the mark.

`initialize` has 30 seconds. A request has 10 seconds. Cancellation
of the tool run sends `$/cancelRequest` for that id and returns
`cancelled`.

### Documents

Robi has no open buffers. The disk is the text the server sees.

A tool opens a file by sending `textDocument/didOpen` with the full
UTF-8 body and the language id from the catalog. Every change sends
the full text again. Before a request, a changed mtime or size sends
`textDocument/didChange` with that text, then `textDocument/didSave`.
The save is what `checkOnSave` uses to run the compiler. A first open
sends `didSave` after `didOpen` for the same reason. Each successful
`write_file`, `edit_file`, or `delete_file` tells the hub. If that
path is open, a write sends `didChange` and `didSave`, and a delete
sends `didClose`. If no server is running, the notice is a no-op.

At most 32 documents stay open per server. The least recently used
gets `didClose`. A file over 1 MiB is not opened. The tool returns
`file is too large`. A non-UTF-8 file returns `file is not utf-8`.

Positions on the wire are LSP's UTF-16. Tool results are 1-based
line and character, counted in Unicode scalar values, matching
`read_file`. The client converts at the boundary.

### What the client advertises

The `initialize` capabilities are the methods v1 calls, and nothing
that would make a server push an editor feature:

- `textDocument/didOpen`, `didChange`, and `didSave`
- `textDocument/publishDiagnostics`, including related information
- `textDocument/definition`, `references`, and `hover`
- `workspace/symbol`
- `workspace/workspaceFolders` and `window/workDoneProgress`

Hover content format is plaintext. Completion, rename, code action,
formatting, semantic tokens, and inlay hints are not advertised.

### Tools

Five tools. A path inside the workspace resolves the way `read_file`
does, then `allows_read` is applied. A denied path is the same tool
error a read returns. A path outside the workspace is
`path is outside the workspace` even after `grant`. The server has
one root. The model reads that other tree with `read_file` and
`grep`.

All five are `Concurrent`. `requires_approval` returns allow
immediately. They are registered in ask, plan, and agent, and in
both `delegate` child registries.

A location the path filter denies is omitted from the result. The
result does not include a file body. A definition or reference
includes one preview line read from disk, clipped to 300 characters,
so the model can see the hit without a second read.

| Tool | Arguments | Result |
|---|---|---|
| `diagnostics` | `path` | Diagnostics for that file. See below |
| `definition` | `path`, 1-based `line`, 1-based `character` | Up to 20 locations. Each is `path`, `line`, `character`, and `preview` |
| `references` | `path`, `line`, `character` | Up to 50 locations, same shape. The declaration is included |
| `hover` | `path`, `line`, `character` | `contents` plaintext, clipped to 8 KB, or an empty string when the server has nothing |
| `workspace_symbol` | `query` | Up to 50 symbols: `name`, `kind`, `path`, `line`, `character`. An empty query is `invalid arguments`. See below |

`diagnostics` is the verification loop. After the document is synced
to the bytes just written, the tool waits until diagnostics for that
URI settle, then returns. Settle means a `publishDiagnostics` for the
current version, then 400 ms with no newer publish for that URI, and
no open progress token. The wait stops at 8 seconds. The result says
`pending: true` when the cap was hit with a token still open or with
no publish yet. A later call returns whatever has arrived. The model
calls once after an edit. It calls a second time only when `pending`
is true.

```json
{
  "available": true,
  "language": "rust",
  "server": "rust-analyzer",
  "path": "crates/robi/src/lib.rs",
  "pending": false,
  "truncated": false,
  "diagnostics": [
    {
      "severity": "error",
      "line": 12,
      "character": 5,
      "message": "cannot find value `missing` in this scope",
      "source": "rust-analyzer",
      "code": "E0425"
    }
  ]
}
```

Severity `error` and `warning` are returned. Hint and information
are dropped. At most 40 diagnostics, errors first. A message is
clipped to 500 characters. Related locations that the path filter
allows are included, up to three, as `path`, `line`, and `message`.

When the server cannot answer, the result is still success:

```json
{
  "available": false,
  "reason": "no_server",
  "language": "rust",
  "server": "rust-analyzer",
  "hint": "rust-analyzer is not on PATH. Use the shell to compile, or grep."
}
```

`definition`, `references`, `hover`, and `workspace_symbol` use the
same `available` and `reason` fields. A miss leaves `locations`,
`contents`, or `symbols` empty. `reason` is `no_server`,
`unsupported`, `server_failed`, or `timeout`. `timeout` on
`diagnostics` still includes any diagnostics already stored, with
`pending: true`. A navigation timeout is `available: false` and an
empty list.

`workspace_symbol` fans out across the catalog. It starts a server
only when two things are true: the binary is on `PATH`, and the
workspace contains at least one file with an extension from that
row. The probe honors `.gitignore`, skips directories the path
filter skips, and stops for a row once one file has matched. A Rust
workspace starts `rust-analyzer` only. A missing binary is left out
of the fan-out. When every present language is missing its binary,
the result is `available: false` and `reason: "no_server"`. When at
least one server answers, `available` is true and `symbols` merges
that server's hits, filtered and capped at 50.

The tool descriptions tell the model to call `diagnostics` on a file
it just edited when the extension has a catalog row, and to use
`shell` when `available` is false. They tell it to use `definition`
and `references` for a symbol, and `grep` for an exact string.
`workspace_symbol` is how it finds a name when it does not know the
file. The agent-mode prompt block gains one sentence with the same
rule, so the instruction survives a registry description the model
skims. That sentence is added when the tools are registered.

### Setting

`lsp` is a global setting. The values are `on` and `off`. An absent
key is `on`, and the first read stores that. `PUT` rejects any other
value. The actor reads the key when it builds the registry. A turn
that is already running keeps the tools it started with.

`off` leaves `diagnostics`, `definition`, `references`, `hover`, and
`workspace_symbol` out of ask, plan, agent, and both child
registries. The agent-mode sentence and the child prompt lines that
name those tools are omitted with them. Writes do not notify a
server. A server that is already running is left to its idle
shutdown. The next call does not start one.

### Surgery stays a later cut

Rename, code action, and format are the third rung of the roadmap
ladder. They are not in the first cut. When they land, they are
agent-mode only, `Exclusive`, and they never let the server write.

The client calls `textDocument/rename` or `textDocument/codeAction`,
reads the `WorkspaceEdit`, and applies each file through the same
write path as `edit_file`: the path filter, the process-wide lock,
a temporary file, a rename, and a checkpoint. A server command
inside a code action is refused. `workspace/applyEdit` stays
`applied: false`. A path the filter denies returns `NeedsApproval`
for the whole edit, the same way a write does. A child never sees
these tools.

## Rejected alternatives

- **A hand-rolled JSON-RPC client over stdio.** Framing and ids are
  the small part. The server's own requests during `initialize` are
  the part that deadlocks a write-only client.
- **`tower-lsp`.** It builds a language server. It does not drive one.
- **Downloading or bundling language servers.** The binary, its
  toolchain, and its index are the user's. A missing binary degrades
  the tool. It does not start a background install.
- **Reading VS Code, Neovim, or Zed settings to find the command.**
  Those files are a second product's config, and they are not a
  stable argv. The catalog is the list. A per-user override file can
  wait until a row is actually wrong.
- **One tool with an `action` enum.** Definition, references, hover,
  symbols, and diagnostics have different arguments and different
  failure text. Separate tools match the rest of the registry.
- **Opening every file, or watching the whole tree, at startup.**
  The server indexes the workspace itself. We send text for files a
  tool actually touches.
- **Putting the server in the shell sandbox.** `cargo` and `go` need
  the toolchain and a real `HOME`. The argv is fixed by the catalog,
  which is what makes an unsandboxed child acceptable.
- **Walking above the workspace root to the Cargo workspace or the
  git root.** Path confinement is the session root. The server may
  look upward on its own. The client does not retarget it.
- **Returning a tool error when no server is installed.** That fails
  a turn that `grep` and `shell` can still finish. `available: false`
  is the signal to fall back.
- **Blocking on a full `cargo check` before the tool returns.** A
  check can take minutes. Eight seconds, then `pending: true`, keeps
  the turn moving. The model calls again when it needs the rest.
- **Applying rename in the first cut.** A workspace edit that skips
  the write path skips the checkpoint. Surgery waits until it can
  use that path.

## Failure modes

- The binary is not on `PATH`. `available` is false, `reason` is
  `no_server`, and the hint names the argv. The turn continues.
- The extension has no row. `reason` is `unsupported`. The model
  does not retry the language server.
- `initialize` exceeds 30 seconds, or the process exits twice.
  `reason` is `server_failed`. The hint includes the server's stderr
  when it wrote any, so a rustup proxy that exits with "Unknown
  binary" is visible to the model. Later calls do not spawn again
  until `robi-api` starts again.
- The path is denied. The tool error matches `read_file`. A path
  outside the workspace is `path is outside the workspace`. The
  server is not told about the file.
- The file is missing, a directory, not UTF-8, or over 1 MiB. The
  tool error says which. Nothing is opened.
- `line` or `character` is past the end of the file. The tool error
  names the argument. The server is not queried.
- A navigation request exceeds 10 seconds. `available` is false,
  `reason` is `timeout`.
- Diagnostics are still running at 8 seconds. The result contains
  what has been published and `pending: true`.
- The server publishes a diagnostic in a file the session may not
  read. That item is omitted.
- Two sessions share one server and both edit. `didChange` carries
  the full text after the write lock is released, so the server's
  copy matches the disk the next request sees.
- The user edits the file in their own editor. The next tool call
  sees a new mtime and sends `didChange` before the request.
- Cancellation mid-request returns `cancelled` and does not count
  as a server failure.
- Starting `rust-analyzer` for a diagnostics call on this repository
  still restarts `make dev-api`. The watcher is `cargo watch` on
  `Cargo.toml`, `Cargo.lock`, `crates/robi`, and `crates/robi-core`.
  The restart is logged as `[Running 'cargo run -p robi --bin robi-api']`
  immediately after the server opens `.rs` files. Ruff does not do
  this: a Python file is outside those paths, and Ruff does not run
  Cargo. The API dies while the Rust calls are still unfinished, so
  they stay `pending` and the chat shows them as approval prompts.
  `cargo.targetDir` and `--locked` did not stop it. Not fixed.
