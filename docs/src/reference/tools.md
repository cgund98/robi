# Tools

These are the tools Robi's assistant can call. Each one is a JSON-schema function
the model sees; its `description` and schema are the main lever on whether it is
called correctly, so they are treated as reviewed copy.

Which tools exist depends on the session's **mode**:

- **Ask** — read-only. Reads, search, language-server lookups, `skill`, web, and
  `grant`.
- **Plan** — the ask set plus `shell`, `retrieve`, and `write_plan` (to author a
  plan).
- **Agent** — the plan set plus the edit tools, `todos`, and `delegate`.

Language-server tools (`diagnostics`, `definition`, `references`, `hover`,
`workspace_symbol`) are registered only when the `lsp` setting is `on`.

## Read and search

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `read_file` | `path`, `offset`, `limit` | Read a text file, one 32 KB window. Reports where to continue. | none |
| `read_code` | `path`, `compress`, `focus_symbols`, `expand_imports`, `depth`, `offset`, `limit` | An outline of a source file, or an exact window of it. | none |
| `list_dir` | `path` | List one directory, non-recursive. | none |
| `find` | `pattern`, `path`, `glob`, `hidden`, `no_ignore` | Find files by path substring or glob. | none |
| `grep` | `pattern`, `path`, `regex`, `hidden`, `no_ignore` | Content search. Uses ripgrep when available. | none |
| `semantic_search` | `query`, `path`, `limit` | Search the workspace by meaning. | none |
| `retrieve` | `id`, `stream`, `offset`, `limit`, `raw` | Read back a stored original of a compressed tool result. | none |

A read is free inside the workspace. Outside it, the read tools still run, but a
path that the policy floor denies becomes a prompt or a `grant`.

The read and search tools cap their results and say when they truncated. A
silently truncated read is how an agent edits the wrong thing, so every boundary
is reported.

## Language server

Registered when `lsp = on`. Each returns `available: false` when no server for
the file's language is installed; the model falls back to `grep` or `shell` and
the turn continues.

| Tool | Arguments | Behavior |
|---|---|---|
| `diagnostics` | `path` | Errors and warnings for one file. |
| `definition` | `path`, `line`, `character` | The definition of the symbol at a position. |
| `references` | `path`, `line`, `character` | Every reference to the symbol. |
| `hover` | `path`, `line`, `character` | Type and documentation at a position. |
| `workspace_symbol` | `query` | Find a symbol by name across the workspace. |

## Edit

Agent mode only. A delete or an edit to a path the session write rules allow runs
immediately; a path they deny becomes an approval prompt for that call.

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `write_file` | `path`, `content` | Create a file or replace its whole contents. | conditional |
| `edit_file` | `path`, `old`, `new`, `replace_all` | Replace one exact snippet. Fails closed on a missing or non-unique match. | conditional |
| `delete_file` | `path` | Delete one text file. Refuses a directory, a missing path, or a non-UTF-8 file. | conditional |

Every edit is recorded as a checkpoint the user can revert, independent of git.
See [Checkpoints](../design/tools/checkpoints.md).

## Shell

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `shell` | `command`, `cwd`, `unsandboxed`, `read_paths`, `write_paths`, `network` | Run a command inside an OS sandbox. | conditional |

A command in the default profile (sandboxed, no extra paths, network `deny`) runs
without a prompt. A prompt appears for any of:

- `unsandboxed: true`,
- a non-empty `read_paths` or `write_paths`,
- `network: "unrestricted"`.

The sandbox is a requirement, not an option: a sandboxed call is refused where no
mechanism exists. macOS uses Seatbelt (`/usr/bin/sandbox-exec`); Linux uses
bubblewrap; Windows has no sandbox, so only an approved `unsandboxed: true` call
runs there. The child gets a constructed `PATH`, a scrubbed environment, and the
workspace root as its working directory. Output is a bounded artifact, not a
stream, and is killed at 120 seconds or on cancel.

## Plan and todos

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `write_plan` | `body`, `plan_name`, `path`, `todos` | Create or overwrite a plan file for the session. | none |
| `todos` | `path`, `clear`, `remove`, `update`, `add` | Patch the todo checklist in a plan file. Agent mode only. | none |

Plans live at `~/.robi/plans/<session_id>/`. The checklist is read back into the
next agent prompt, so the open item is in front of the model again.

## Delegate

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `delegate` | `task`, `mode`, `description`, `thoroughness`, `instructions` | Hand a bounded task to a child agent and get only its answer. | none |

Agent mode only. `mode` is `explore` (read-only) or `general` (read plus a
sandboxed shell). Neither child can edit, `grant`, `delegate`, or call MCP tools.
A child call that would need approval fails closed with `access_denied`; the user
is never prompted from inside a child. See [Subagents](../concepts/subagents.md).

## Web

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `web_search` | `query` | Brave web search, up to five hits. | always |
| `web_fetch` | `url` | Fetch one public URL and reduce it to text. | first call per host |

`web_search` asks on every call because each spends Brave quota. `web_fetch` asks
the first time a host is used in the session, then remembers that host. Page text
and snippets are untrusted data; an instruction inside a fetched page is data,
not a command.

## Skills and grants

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `skill` | `name` | Load one skill by id and return its files and body. | none |
| `grant` | `path`, `access` | Ask the user to allow a path for this session. | always |

`skill` is registered in every mode. `grant` appends one read or write allow for a
path the session currently refuses, including a path outside the workspace. The
allow is session-scoped and never widens the policy floor for a subagent.

## MCP

Tools from an MCP server are registered as `mcp_<server>_<tool>`. Each asks on
its first call in the session; an approved pair runs on later calls. Client-side
tool names outside `[A-Za-z0-9_-]` are replaced with `_`. See
[MCP](../concepts/mcp.md).

## Where this is specified

Per-tool detail is in the design docs: [Read tools](../design/tools/read-tools.md),
[Editing tools](../design/tools/editing-tools.md), [Shell tool](../design/tools/shell-tool.md),
[Web tools](../design/tools/web-tools.md), [Code outline](../design/tools/code-outline.md),
[Subagents](../design/core/subagents.md), and [MCP](../design/reach/mcp.md).
