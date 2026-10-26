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
| **Plan** | the ask set, with a shell that may write only `.robi/plans` |
| **Agent** | the ask set plus `delegate` and MCP tools; the shell writes the workspace |

Ask holds a **read-only** shell: the sandbox mounts the workspace read-only, so
a command that writes fails inside it. Plan's shell is read-only too, except for
one directory: `.robi/plans`. Agent's shell writes the whole workspace. In Ask
and Plan the schema has no `write_paths` and no `unsandboxed`, and `network` is
restricted to `deny`. Build and test commands write to `target/`, so they fail
there too: verify in Agent mode.

Some tools are compiled out and restored by a Cargo feature. See
[Feature switches](../design/core/feature-switches.md).

| Feature | Restores |
|---|---|
| `read-tools` | `read_file`, `list_dir`, `find`, `grep` |
| `edit-tools` | `write_file`, `edit_file`, `delete_file` |

The semantic index, `semantic_search`, the five language-server tools, and
`read_code` are not features: they are deleted (S3 and S4).

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
| `list_dir` | `path` | List one directory, non-recursive. | none |
| `find` | `pattern`, `path`, `glob`, `hidden`, `no_ignore` | Find files by path substring or glob. | none |
| `grep` | `pattern`, `path`, `regex`, `hidden`, `no_ignore` | Content search. Uses ripgrep when available. | none |

`grep` and `find` are the retrieval path. There is no search by meaning: the
semantic index is gone.

## Verification

Verification is the shell: `cargo check`, `tsc --noEmit`, `eslint`, and the
project's own test command. The language-server tools are gone too.

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

## Plans

There is no plan tool. A plan is a directory in the workspace the agent writes
with the shell:

```
.robi/plans/
  .gitignore                 # one line: *
  ship-mode-registry/
    PLAN.md                  # the prose: what changes, why, in what order
    TODO.md                  # the checklist, and nothing else
```

| Mode | Writes |
|---|---|
| **Ask** | nothing |
| **Plan** | only `.robi/plans`, recursively |
| **Agent** | the workspace, including `.robi/plans` |

One directory per plan, named from the plan's title in kebab case; the prompt
tells the agent to list the directory first and update an existing plan rather
than mint a near-duplicate. `TODO.md` is one line per step, `- [ ]` open and
`- [x]` done, and `PLAN.md` carries no checkboxes — so ticking a step changes
only the checklist. `.robi/plans/.gitignore` holds a single `*`, which ignores
the whole directory including itself: nothing in it is tracked, and deleting
that file is the opt-in to committing a plan.

Because `.robi` is hidden and git-ignored, `git status`, `rg`, and the
`find`/`grep` tools skip it; only `ls` and `cat` see it. See
[S9](../streamlining.md#s9--no-built-in-todos) for why the tools went.

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
[Web tools](../design/tools/web-tools.md), [Subagents](../design/core/subagents.md),
[Feature switches](../design/core/feature-switches.md), and
[MCP](../design/reach/mcp.md).
