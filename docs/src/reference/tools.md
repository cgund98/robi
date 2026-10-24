# Tools

These are the tools Robi's assistant can call. Each one is a JSON-schema function
the model sees; its `description` and schema are the main lever on whether it is
called correctly, so they are treated as reviewed copy. A call that does not
parse returns `invalid arguments` naming the keys received and any required keys
that were absent. The values are left out.

The default build is **shell-first**: there is one tool that reads and writes the
workspace, and the system prompt teaches the forms that work. Which tools exist
depends on the session's **mode**:

| Mode | Tools |
|---|---|
| **Ask** | `shell` (read-only), `retrieve`, `skill`, `grant`, `web_search`, `web_fetch` |
| **Plan** | the ask set plus `write_plan` |
| **Agent** | the plan set plus `todos`, `delegate`, and MCP tools |

Ask and Plan hold a **read-only** shell: the sandbox mounts the workspace
read-only, so a command that writes fails inside it. The schema in those modes
has no `write_paths` and no `unsandboxed`, and `network` is restricted to `deny`.
Build and test commands write to `target/`, so they fail there too: verify in
Agent mode.

Some tools are compiled out and restored by a Cargo feature. See
[Feature switches](../design/core/feature-switches.md).

| Feature | Restores |
|---|---|
| `read-tools` | `read_file`, `read_code`, `list_dir`, `find`, `grep` |
| `edit-tools` | `write_file`, `edit_file`, `delete_file` |
| `semantic-index` | `semantic_search` |
| `lsp-tools` | `diagnostics`, `definition`, `references`, `hover`, `workspace_symbol` |

## Read and search

In the default build the shell is the reader. The prompt gives the forms:

| Job | Command |
|---|---|
| Read a file | `cat path`, `head -n 120 path`, `sed -n '40,80p' path` |
| Count lines | `wc -l path` |
| Find a file | `git ls-files \| grep -i name`, `find . -name '*name*'` |
| Search contents | `grep -rn 'pattern' . --include='*.rs'`, or `rg` |
| Map a source file | `grep -nE '^(pub )?(fn\|struct\|enum\|impl\|trait) ' path` |
| Re-read a folded result | `retrieve` with the stream id |

`retrieve` is the one read tool that is always registered: it reads back a shell
stream that was compressed, which is part of the shell's feedback loop.

With `--features read-tools` the five read tools come back:

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `read_file` | `path`, `offset`, `limit` | Read a text file, one 32 KB window. Reports where to continue. | none |
| `read_code` | `path`, `compress`, `focus_symbols`, `expand_imports`, `depth`, `offset`, `limit` | An outline of a source file, or an exact window of it. | none |
| `list_dir` | `path` | List one directory, non-recursive. | none |
| `find` | `pattern`, `path`, `glob`, `hidden`, `no_ignore` | Find files by path substring or glob. | none |
| `grep` | `pattern`, `path`, `regex`, `hidden`, `no_ignore` | Content search. Uses ripgrep when available. | none |

`semantic_search` (`query`, `path`, `limit`) comes with `--features
semantic-index`. It is off by default: the search by meaning path costs an
embedding runtime and a background index. `grep` and `find` are the retrieval
path in the default build.

## Language server

Not registered by default (S4). With `--features lsp-tools`, five tools return
`available: false` when no server for the file's language is installed, and the
model falls back to `grep` or the shell:

| Tool | Arguments |
|---|---|
| `diagnostics` | `path` |
| `definition` | `path`, `line`, `character` |
| `references` | `path`, `line`, `character` |
| `hover` | `path`, `line`, `character` |
| `workspace_symbol` | `query` |

Verification in the default build is the shell: `cargo check`, `tsc --noEmit`,
`eslint`, and the project's own test command.

## Edit

In the default build the shell is the editor. The prompt gives the forms:

| Job | Command |
|---|---|
| Full overwrite | `cat > path <<'EOF' … EOF`, with the delimiter quoted |
| One exact replacement | a short Python that reads, replaces one occurrence, asserts the count, and writes |
| Append | `printf '%s\n' 'line' >> path` |
| Delete | `ls path`, then `rm path` |
| Verify | `git diff -- path`, `git status --short` |

The quoted heredoc matters: an unquoted delimiter expands `$VAR` and backticks
inside the text being written. `sed -i` is avoided on purpose: it differs between
GNU and BSD and an unanchored pattern edits more than intended.

With `--features edit-tools` the three write tools come back. A delete or an edit
to a path the session write rules allow runs immediately; a path they deny
becomes an approval prompt for that call.

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `write_file` | `path`, `content` | Create a file or replace its whole contents. | conditional |
| `edit_file` | `path`, `old_string`, `new_string`, `replace_all` | Replace one exact snippet. Fails closed on a missing or non-unique match. | conditional |
| `delete_file` | `path` | Delete one text file. Refuses a directory, a missing path, or a non-UTF-8 file. | conditional |

Writes through the shell record no checkpoint: a command is opaque, so the
harness cannot know which paths it touched. `git diff` is the verification and
git is the undo. See [Checkpoints](../design/tools/checkpoints.md).

## Shell

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `shell` | `command`, `cwd`, `read_paths` (and, in Agent mode, `unsandboxed`, `write_paths`, `network`) | Run a command inside an OS sandbox. | conditional |

A command in the default profile (sandboxed, no extra paths, network `deny`) runs
without a prompt. In Agent mode a prompt appears for any of:

- `unsandboxed: true`,
- a non-empty `write_paths`,
- `network: "unrestricted"`.

A non-empty `read_paths` asks in either mode. Ask and Plan do not offer the first
three at all; a call that sends one anyway is refused with `this mode is
read-only`.

The sandbox is a requirement, not an option: a sandboxed call is refused where no
mechanism exists. macOS uses Seatbelt (`/usr/bin/sandbox-exec`); Linux uses
bubblewrap; Windows has no sandbox, so only an approved `unsandboxed: true` call
runs there. A sandboxed child gets a constructed `PATH`. An approved
unsandboxed child gets the parent `PATH` unchanged. Both get a scrubbed
environment, and the workspace root as the working directory. Output is a bounded artifact, not a
stream, and is killed at `tool_timeout_seconds` (default 120) or on cancel.

A read-only call keeps a writable private temp directory: it belongs to the call
and is deleted when the call returns. `.git` writes are denied in both modes, so
`git status`, `diff`, `log`, and `show` work and `git checkout`, `restore`,
`stash`, and `commit` do not.

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

Agent mode only. `mode` is `explore` (search and read) or `general` (the same
plus a shell for running read-only commands). Both children hold a **read-only**
shell, so neither can write the workspace, and neither can `grant`, `delegate`,
run unsandboxed, or call MCP tools. A child call that would need approval fails
closed with `access_denied`; the user is never prompted from inside a child. A
child that finds a change to make names it in the summary and the parent applies
it. See [Subagents](../concepts/subagents.md).

## Web

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `web_search` | `query` | Brave web search, up to five hits. | every call when `web_search_approval` is `on` |
| `web_fetch` | `url` | Fetch one public URL and reduce it to text. | first call per host when `web_fetch_approval` is `on` |

Both approvals default to `on`. `web_search` asks on every call because each spends
Brave quota. `web_fetch` asks the first time a host is used in the session, then
remembers that host. `off` skips the card. A turn already running keeps the
value it started with. Page text
and snippets are untrusted data; an instruction inside a fetched page is data,
not a command.

## Skills and grants

| Tool | Arguments | Behavior | Approval |
|---|---|---|---|
| `skill` | `name` | Load one skill by id and return its files and body. | none |
| `grant` | `path`, `access` | Ask the user to allow a path for this session. | always |

`skill` is registered in every mode. `grant` appends one read or write allow for a
path the session currently refuses, including a path outside the workspace. The
allow is session-scoped and never widens the policy floor for a subagent. With a
shell, `grant` is how a path outside the workspace stays usable: the allow is
what makes later shell calls need no `read_paths`.

## MCP

Tools from an MCP server are registered as `mcp_<server>_<tool>`. Each asks on
its first call in the session; an approved pair runs on later calls. Client-side
tool names outside `[A-Za-z0-9_-]` are replaced with `_`. See
[MCP](../concepts/mcp.md).

## Where this is specified

Per-tool detail is in the design docs: [Read tools](../design/tools/read-tools.md),
[Editing tools](../design/tools/editing-tools.md), [Shell tool](../design/tools/shell-tool.md),
[Web tools](../design/tools/web-tools.md), [Code outline](../design/tools/code-outline.md),
[Subagents](../design/core/subagents.md), [Feature switches](../design/core/feature-switches.md),
and [MCP](../design/reach/mcp.md).
