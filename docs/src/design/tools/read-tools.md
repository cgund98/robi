# Read tools

This page defines the read-only workspace tools and the session path
filter they share. It is the design for **F3.1** in the
[roadmap](../../roadmap.md). Approval cards stay in `docs/src/design/workspace/permissions.md`.
The shell sandbox stays in `docs/src/design/tools/shell-tool.md`. Schema and `PATCH`
stay in [persistence.md](../persistence/persistence.md).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Per-call approval and session grants | `docs/src/design/workspace/permissions.md` (M3) |
| OS sandbox for a shell tool | `docs/src/design/tools/shell-tool.md` (M4) |
| Compaction when a read fills the window | `docs/src/design/workspace/context-management.md` (M3) |
| How the actor is built | [chat-runtime.md](../shell/chat-runtime.md) |
| An outline that folds function bodies | [code-outline.md](code-outline.md). `read_file` stays an exact window |

## Problem

The model has to read the workspace, and a read has to stay inside that
workspace. Secret files and `.git` should be left out unless the session
says otherwise. Those exceptions differ per chat, so they belong on the
session row. Content search should use ripgrep when it is installed, and
still work when it is not.

## Decision

`read_file`, `read_code`, `list_dir`, `find`, `grep`, and `grant` live in
`crates/robi/src/agent/tools/`. Each read resolves its path, then asks a
`PathFilter` compiled from that session's rules. `grant` is exclusive and
always needs approval. It appends one allow for the refused path, `read` or
`write`, including a path outside the workspace such as `../gopi`. A directory
pattern matches that directory and its children. The workspace root is `^.*$`,
which loses to any deny that names literals. The outside-workspace deny is
`^\.\.(/|$)`, so `../gopi` reaches further and a sibling stays denied.

The filter is four lists of Rust regexes, matched against the
workspace-relative path (`/` separators, no leading slash):

| List | Effect |
|---|---|
| `deny_read` | A matching read deny. Loses to a more specific read allow |
| `allow_read` | A matching read allow. Outranks a deny that reaches less far, or the same point with fewer literals |
| `deny_write` | A matching write deny. Loses to a more specific write allow |
| `allow_write` | A matching write allow. Ranked the same way as `allow_read` |

When several patterns match one path, the match that ends furthest into
the path wins. If two matches end on the same byte, the pattern with more
literal characters wins. Wildcards, anchors, and groups do not add to that
count. A deny wins a remaining tie. Allowing `gopi` leaves `gopi/.git`
denied, because the `.git` rule reaches further. Allowing `gopi/.git`
reaches the same point and names more of the path, so that directory and
its children are readable. A wildcard such as `^src/.*$` does not outrank
the literal `.env` deny. An empty allow list means there are no exceptions.
A write allow that beats a write deny also permits the read. An open write
with no matching write deny does not. `allows_read` and `allows_write`
are the methods a later shell sandbox calls. `skip_dir` is true when a
non-root directory fails `allows_read`, so a walk does not descend into
`.git`.

The built-in deny patterns live in source and are always applied. Create
stores `[]` for all four lists. A stored list is appended after that built-in
list. Allow lists have no stored built-in patterns. `path_allow_read` and
`path_allow_write` in `~/.robi/config.toml` are newline-separated paths.
The factory gives each new agent the settings store, and the filter reads
those two keys from it on every call. `path_entries` is appended to the
read allow list the same way. A line starting with `~/` expands to
the home directory. Each path is appended to that session's allow list: a
file is an exact allow, and any other path is that directory and its
children. Compiling a filter for a
session adds one read allow for `~/.robi/plans/<session_id>`, matched on the
workspace-relative path, including a path that starts with `..`. That allow
covers the directory and the markdown files in it. It does not cover another
session's plans, the rest of `~/.robi`, or writes. `edit_file` still cannot
change a plan. Both deny lists start from:

- `^\.\.(/|$)` — a workspace-relative path that starts with `..`
- `(^|/)\.git(/|$)`
- `(^|/)\.env$` and `(^|/)\.env\.[^/]+$`
- `(^|/)[^/]+\.(pem|key)$`
- `(^|/)id_rsa$` and `(^|/)id_ed25519$`
- `(^|/)credentials\.json$` and `(^|/)secrets\.json$`

`PATCH` replaces the stored addition. Clearing a deny list leaves the
built-in patterns in place. A more specific allow still overrides a built-in
deny. A pattern that does not compile is `400` and is not stored.

`Tool::execute` does not receive a session id, and the trait stays as M0
fixed it. The actor builds a registry whose tools close over the session.
Each call reloads the lists, so a `PATCH` applies on the next tool call.
The same registry is what `ModelSource::model` offers the provider.
`requires_approval` returns allow immediately for the read tools. `grant`
always needs approval. A denied path, including one outside the workspace,
is a tool error on `read_file`, `list_dir`, and the start path of `find` and
`grep`. A denied file under an allowed directory is omitted from `find` and
`grep`. After `grant` saves an allow such as `../gopi`, later calls may read
that tree. A more specific deny, such as `.git`, still wins.

### Path arguments

Every path argument goes through `workspace::resolve_path` before a tool
reads it.

1. A leading `~` or `~/` expands to `$HOME`. `~otheruser` is not expanded.
   It is a relative path whose first component is the literal `~otheruser`.
2. A relative argument, including `./` and `../`, joins the workspace root.
3. An absolute argument is used as given.
4. `.`, `..`, and symlinks are resolved. A path outside the canonical
   workspace root is kept. Its relative form starts with `..`, which the
   built-in deny matches until a more specific allow does.

A search pattern or a glob is not a path and is not expanded. The same
helper is what a later shell uses for its working directory and path
arguments.

### Tools

| Tool | Arguments | Result |
|---|---|---|
| `read_file` | `path`, optional 1-based `offset`, optional `limit` | `content`, `start_line`, `end_line`, `total_lines`. At most 32 KB. When `truncated` is true, `next_offset` is the line to pass next |
| `read_code` | `path`, optional `compress`, `focus_symbols`, `depth`, `expand_imports`. `offset` and `limit` only when `compress` is false | An outline (`view: "outline"`) or an exact window (`view: "source"`). See [code-outline.md](code-outline.md) |
| `list_dir` | optional `path`, defaulting to the workspace root | One level of `name` and `kind` (`file`, `dir`, or `symlink`). Denied children are omitted. `.gitignore` is not applied |
| `find` | optional `pattern`, optional `path`, optional `glob`, optional `hidden`, optional `no_ignore` | Workspace-relative files. `glob` false matches a substring. `glob` true matches a glob. At most 50 files. Honors `.gitignore` and skips hidden files unless `no_ignore` or `hidden` is set. Symlinks are not followed |
| `grep` | `pattern`, optional `path`, optional `regex`, optional `hidden`, optional `no_ignore` | `matches` of `path`, `line`, and `text`, plus `backend` |

`find` walks with the `ignore` crate using ripgrep's defaults: `.gitignore`,
global gitignore, `.ignore`, and hidden files. `hidden` and `no_ignore`
mean the same thing they do for `grep`. A denied path is still omitted
when those toggles are on.

`grep` looks for `rg` on `PATH`. When it is there, the process is
`--json`, with `-F` unless `regex` is true, and `-e` so a pattern that
starts with `-` is not a flag. Ripgrep's own defaults stay on, including
`.gitignore`, hidden files, and binary files. `hidden` adds `--hidden`.
`no_ignore` adds `--no-ignore`. The model cannot pass a raw argument
string.

A deny-read pattern of the form `(^|/)name(/|$)` becomes an `rg --glob`
exclusion (`!**/.git/**` for the default `.git` rule) so ripgrep does not
walk that directory. The glob is omitted when an `allow_read` pattern
matches a path inside that directory. Every match is still checked with
`allows_read`, including when `no_ignore` is true, so a gitignored `.env`
stays out of the result.

When `rg` is missing, or the process fails to spawn, `grep` walks with
the `ignore` crate and sets `fallback_reason`. The same two booleans
apply. A non-zero ripgrep status other than "no matches" is a tool error
and does not fall back. The walker skips a directory `skip_dir` rejects,
skips a file that contains a NUL in its first 8 KB, and clips a long line
to about 300 characters around the match.

Both backends stop at 50 matches or 32 KB of match text. `truncated` is
then true, and `hint` says to narrow the path or pattern and call again.
`backend` is `ripgrep` or `builtin`.

All four tools are `Concurrent`.

## Rejected alternatives

- **One allow list for both directions.** A path that may be read is not
  the same decision as a path that may be written. The shell and the edit
  tools need the split.
- **An immutable deny floor under the session lists.** The lists are the
  policy the user edits. A floor that survives `PATCH` would make "saved
  and modified" mean something else. Approval for protected paths is a
  later gate, not a second copy of these lists.
- **Forcing `--hidden` and `--no-ignore`.** That throws away ripgrep's
  `.gitignore` behavior. The two flags are opt-in booleans instead.
- **A free-form `rg` argument string.** The model could pass a flag that
  reads outside the workspace. The schema exposes the two booleans.
- **The `ignore` crate as the only search engine.** Ripgrep is faster on
  a large tree. The crate is the fallback so a machine without `rg` still
  searches, and so that fallback honors `.gitignore` the same way.
- **Putting the tools in `robi-core`.** They read the filesystem. The
  loop crate stays free of I/O. The trait signatures stay fixed, and the
  session is closed over at the actor.

## Failure modes

- A path outside the workspace, including one reached through a symlink, is
  `path is not allowed` until a grant allow matches it. The relative form
  starts with `..`.
- A denied start path is `path is not allowed`, plus the workspace-relative
  path. Contents are not returned.
- A missing file is `file not found`. A directory passed to `read_file` is
  `path is a directory`. A non-UTF-8 file is `file is not utf-8`.
- An empty `grep` pattern, a regex that does not compile, or a glob that
  does not compile is `invalid arguments`. The tool does not run.
- `$HOME` unset makes a `~/` argument fail before any read.
- Cancellation drops the ripgrep child (`kill_on_drop`) and returns
  `cancelled`.
- A stored list that no longer compiles (a row written by something other
  than `PATCH`) fails the tool call. `PATCH` is what keeps new lists valid.
