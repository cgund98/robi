# Code review

This page is the review of files a chat session has changed. The
baselines it reads are in [checkpoints.md](checkpoints.md). The chat strip
that opens it is in [visual-style.md](visual-style.md).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Inline comments, and sending a comment to the assistant | Later in M6 |
| A tool the assistant uses to read review state | Later in M6 |
| Git status, a commit range, or a branch | Later. This screen is the session baseline only |
| Side-by-side columns | Later. This screen is one unified column, with a control that hides one side |

## Problem

After `write_file`, `edit_file`, or `delete_file`, the transcript should not
become a pile of patches. The user needs a stable place, tied to this chat
session, that lists the paths and shows what changed.

## Decision

`GET /api/v1/chat_sessions/{id}/review` loads the session, its workspace
root, and the baselines for that session. Each file is diffed against the
bytes on disk. A path with zero additions and zero deletions is left out. A
file this session created that is already gone is left out, the same way
`hunks_for_session` omits it.

The response is `{ files }`. Each file has `path`, `status` (`added`,
`deleted`, or `modified`), `additions`, `deletions`, `baseline`, `current`,
and `lines`. A line is `context`, `delete`, `insert`, or `gap`. `old_line`
and `new_line` are 1-based, and both are null on a gap. Context is three
lines. A gap is the unchanged stretch between two hunks that did not merge.
`baseline` and `current` are the full texts, so the client can highlight
them. Deletions are painted from the baseline. Context and insertions are
painted from the current text.

The chat shell keeps one line above the composer when `files` is not empty:
`N file(s) edited`, and **Review**. It refetches when the session is
selected and when `tool_call_updated` or `turn_finished` arrives. **Review**
goes to `#/sessions/:id/review`. That route keeps the session sidebar. The
main column is the review. **Back** returns to the chat for that session.
Choosing another session, or New chat, leaves the review.

The left pane is a tree of the changed paths. At each level, directories
come first, then files. Each group is alphabetical, without regard to case.
A click scrolls the right pane to that file.

The file header shows the path, then `+N` and `-N`. **Reject** and **Approve**
sit on the right. A click on the path opens that file in a dialog. The dialog has
**Diff**, **Current**, and **Previous**, and it opens on **Diff**. **Diff**
is the whole file, with deletions and insertions in place. Unchanged
lines stay in the list. **Current** is the whole current text.
**Previous** is the whole baseline. A markdown file also has **Preview**,
which renders the current text. Headings step down by level. A deleted
markdown file renders the baseline. An added file on **Previous**, or a
deleted file on **Current**, shows `File added` or `File deleted`. **Approve** keeps the file as it is and folds that text
into the baseline, so the path leaves the review. **Reject** writes the
baseline back. A file this session created, fully rejected, is removed.

Each block between gaps is a hunk. Hovering it shows **Reject** and
**Approve** on the top right of that block's first changed line, except on a file this session
created: that file only has the buttons on its header. **Approve** folds that hunk's
current lines into the baseline. **Reject** puts that hunk's baseline lines
back on disk. A hunk that no longer matches the file is `409`.

The right pane lists every file in that tree order. Each file is its own
panel, with space between panels. The default is the
unified diff: context, deletions, and insertions. **Current** hides
deletions. **Previous** hides insertions. An added file on Previous, or a
deleted file on Current, shows one line — `File added` or `File deleted` —
instead of an empty block. A long line scrolls with the whole file, in
one horizontal scroll. The line numbers stay put. Copying lines leaves
the numbers and the `+` / `−` marks out.

Known extensions (`ts`, `tsx`, `js`, `jsx`, `rs`, `py`, `go`, `json`, `css`,
`md`, `html`, `toml`, `yaml`, `yml`, `sql`) are highlighted with the
`github-dark` theme. Anything else stays in `--code-ink`.

## Rejected alternatives

- **A per-file card in the transcript.** The strip would grow with the
  session, and the diffs would scroll away with the messages. The count
  stays put. The diffs live on the review route.
- **Putting review lines on `FileDiff`.** The edit tools already return a
  patch and hunks to the model. Review lines, with context and gaps, are a
  separate type so that tool result does not change.
- **Highlighting each diff line on its own.** A comment or a string that
  crosses a line would be wrong. The full baseline and the full current
  text are tokenized, then each visible line takes the tokens for its
  number.

## Failure modes

- A non-UTF-8 file is `400`, the same as `hunks_for_session`.
- A missing session is `404`.
- A failed review fetch leaves the strip hidden when it has no files yet.
  The review screen shows the error.
- Hiding both sides is not a state. **Current** and **Previous** each hide
  one side.
