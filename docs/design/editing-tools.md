# Editing tools

This page settles **D5** and defines the three write tools. It is the design
for **F4.1** in the [roadmap](../roadmap.md). The session baseline those writes
record is [checkpoints.md](checkpoints.md). Path rules are in
[read-tools.md](read-tools.md). The approval card the loop already shows is in
[chat-ui.md](chat-ui.md).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Putting a hunk back, and the review window | `docs/design/code-review.md` (M6) |
| The shell sandbox | `docs/design/shell-tool.md` (M4) |
| Diagnostics after a write | `docs/design/lsp.md` (M7) |
| A saved allow for the rest of the session | `grant`, in [read-tools.md](read-tools.md) |
| Refusing an outline marker in `old`, `new`, or `content` | [code-outline.md](code-outline.md) |

## Problem

The model has to change files, and a wrong change has to be visible and
reversible. A unified diff the model authors looks like a review, but it
applies to the wrong place when the file has drifted, and models emit
malformed patches. A fuzzy match has the same failure in a quieter form: the
write succeeds when `old` was not the bytes in the file.

## Decision

Three tools, `write_file`, `edit_file`, and `delete_file`, live in
`crates/robi/src/tools/`. Each call resolves its path the same way a read
does. All three are `Exclusive`.

The model edits with exact text. `edit_file` replaces `old` with `new`. It
does not accept a patch. `write_file` creates a file or replaces its whole
body. `delete_file` removes one file. After a successful change, the tool
returns a file diff of that change: the workspace-relative path, a unified
patch, `additions`, `deletions`, and `added`, `deleted`, or `modified`. The
patch tells the model what landed. It is not the edit instruction.

`old` is matched exactly, after the model's `\n` is converted to the file's
line ending (`\n` or `\r\n`). A UTF-8 BOM already on the file is kept.
`replace_all` defaults to false. Set, it replaces every exact match. Unset,
more than one match is an error. `old` and `new` must differ. An empty `old`
is an error that names `write_file`.

A path `allows_write` accepts runs immediately. A path the filter denies
returns `NeedsApproval`. That includes a built-in deny such as `.env` or
`.git`, and a path outside the workspace. The loop pauses and the existing
approval card shows Write or Delete with the path. A pending `edit_file`
shows the same diff card as a finished edit, from `old` and `new`, with the
same line cap. Approval is for
this call. It does not append an allow rule. The next write to that path
asks again. A lasting exception stays on `grant`.

`execute` does not receive the decision. The loop calls it only after
approval, or immediately when the filter allows the path. The write then
proceeds even though the filter still denies the path. Every other check
still applies. An argument that does not parse, or a path that does not
resolve, returns `AllowImmediately` so `execute` reports the error and the
user is not prompted.

A process-wide lock, keyed by the canonical path, is held for the whole
read-modify-write. The session actor already serializes one session. The
lock covers two sessions editing one file. The bytes are written to a
temporary file in the same directory, then renamed.

| Tool | Arguments | Behavior |
|---|---|---|
| `write_file` | `path`, `content` | Create the file, or replace its whole body. Parent directories under the resolved path are created |
| `edit_file` | `path`, `old`, `new`, optional `replace_all` | Replace one exact occurrence, or every occurrence when `replace_all` is true |
| `delete_file` | `path` | Remove one file. A directory is refused. Parent directories stay |

## Rejected alternatives

- **A unified diff as the edit instruction.** The review preview is computed
  from the session baseline. The model does not have to author it. A patch
  applies to the wrong place when the context has drifted.
- **Fuzzy replacement.** Trimmed lines, folded whitespace, flexible
  indentation, and similarity scoring all succeed when `old` was not the
  bytes in the file. A miss stays an error. When the text matches only after
  trimming each line, the error says so.
- **One tool that both creates and edits.** An empty `old` is how a model
  accidentally overwrites a file. Creation and full replacement are
  `write_file`.
- **Refusing a denied path with no prompt.** The user can allow one write
  without saving a session rule. `grant` remains the way to keep the path
  allowed.
- **Saving an allow when the user approves a write.** That would make one
  approval a lasting exception. The next call asks again unless the model
  calls `grant`.

## Failure modes

- `old` and `new` are identical: `old and new are identical`.
- `old` is empty: `old is empty; use write_file to create or replace the file`.
- `old` matches nothing: `old text was not found`, and a second sentence when
  the text matches only after trimming each line.
- `old` matches more than once and `replace_all` is false: `old text matched N times`.
- A missing file on `edit_file` or `delete_file` is `file not found`. A
  directory is `path is a directory`. A non-UTF-8 file is `file is not utf-8`.
- A path that resolves fails before any write, with the argument the model
  passed. `$HOME` unset makes a `~/` argument fail the same way.
- Cancellation before the read returns `cancelled` and does not write.
- Rejecting the approval card does not call `execute`. The file stays as it was.
