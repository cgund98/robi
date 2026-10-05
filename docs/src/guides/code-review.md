# Code review

Once the assistant edits files, the changes do not pile up in the transcript as a
list of patches. They collect on a **review screen** tied to the chat session:
one stable place that lists every path the session changed and shows the diff,
with controls to keep or discard each change.

Review reads the session **baseline** — the bytes as they were when the session
started (or when a change was last approved). It diffs that against the bytes on
disk. A file with no additions and no deletions is left out, so the list holds
only paths that actually differ.

## Open the review

When a session has changed at least one file, a strip sits just above the
composer:

> **N files edited**  **Review**

Click **Review** to open `#/sessions/:id/review`. The session sidebar stays in
place; the main column becomes the review. The header's back button is
`← <session title>` and returns you to that session's chat. Choosing another
session, or **New chat**, leaves the review.

The strip refetches as the session runs, so the count rises as the assistant
edits more files. If nothing has changed, there is no strip.

## The screen

Two panes:

- **Left — the changed-path tree.** Directories first, then files, each group
  alphabetical and case-insensitive. Click a file to load it in the right pane.
  Only the selected file is fetched and painted.
- **Right — the diff.** The default is a **unified diff**: unchanged context,
  `−` deletions, `+` insertions. Long lines scroll horizontally with the whole
  file, and the line numbers stay put.

A header toggle switches the view:

| View | Shows |
|---|---|
| **Diff** | Context, deletions, and insertions — the full change |
| **Current** | The file as it is now (insertions and context; deletions hidden) |
| **Previous** | The file as it was (deletions and context; insertions hidden) |

On a file the session created, **Previous** shows `File added`; on a deleted
file, **Current** shows `File deleted`. Context is three lines around each
change, and `···` marks the unchanged stretch between two hunks that did not
merge.

Known extensions (`ts`, `tsx`, `js`, `jsx`, `rs`, `py`, `go`, `json`, `css`,
`md`, `html`, `toml`, `yaml`, `yml`, `sql`) are syntax-highlighted with the
`github-dark` theme. Anything else renders in plain `--code-ink`.

**Open the whole file.** Click the path in a file's header to open a dialog with
**Diff**, **Current**, and **Previous** tabs (it opens on **Diff**); a markdown
file also gets **Preview**, which renders the current text.

## Approve and reject

Each file header shows the path, then `+N` / `-N` on the left and **Reject** /
**Approve** on the right. Both work at two scopes:

- **Whole file** — the header buttons.
- **One hunk** — hover a block between gaps; **Reject** and **Approve** appear on
  the first changed line of that block. A file the session created only has the
  buttons on its header.

The two decisions mean:

- **Approve** keeps the file (or hunk) as it is and folds that text into the
  baseline, so it leaves the review. An approved path leaves the list as soon as
  you click. If the request fails, the file comes back and the error is shown.
- **Reject** writes the baseline back to disk. A file the session created that
  is fully rejected is removed.

Deciding one hunk reloads only that file. A hunk that no longer matches the file
on disk returns a conflict instead of applying a stale change.

## When the list is empty

- **No files changed in this session.** — nothing to review.
- **Loading review…** / **Loading file…** — a fetch is in flight.
- A non-UTF-8 file cannot be diffed and is reported as such.

## Where this is specified

The routes, the tree ordering, the hunk decisions, and the failure modes are in
[Code review](../design/review/code-review.md). The baselines it reads are in
[Checkpoints](../design/tools/checkpoints.md).

## Next

- [Approvals](../concepts/approvals.md) — what prompts before an edit runs.
- [Tools](../reference/tools.md) — the edit tools that produce a review.
