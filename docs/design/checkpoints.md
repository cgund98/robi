# Checkpoints

This page defines the edit journal for **F4.2** in the
[roadmap](../roadmap.md). The tools that write the journal are in
[editing-tools.md](editing-tools.md). The table is in
[persistence.md](persistence.md). Putting a hunk back is
`docs/design/code-review.md` (M6).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Approve and revert in the review window | `docs/design/code-review.md` (M6) |
| Git status, commit, and history | Later, with the shell tool |
| The approval card for a denied path | [editing-tools.md](editing-tools.md) |

## Problem

An agent that can write needs a way back that does not depend on git. The
working tree may be dirty before the session starts, and a revert has to
restore what this session changed, not whatever `git checkout` would do.

## Decision

Each chat session stores one baseline per path it changes. The baseline is
the file's UTF-8 body before this session's first `write_file`, `edit_file`,
or `delete_file` of that path. A file that did not exist stores `''` and
`created`. The row is inserted before the rename or the remove, and a later
change of the same path does not replace it.

`crates/robi/src/review/` diffs that baseline against the file now on disk.
`hunks_for_session` returns one file diff per path: status, additions,
deletions, a unified patch, and the hunks inside it. A hunk is an id, the
baseline line start, the current line start, the old lines, and the new
lines. Line starts are 0-based. The id is the first 8 bytes of SHA-256 over
the baseline start and both line lists, hex encoded. A missing file is
status `deleted`: one hunk whose new lines are empty. `reject` puts that
hunk's old lines back and fails when the current lines no longer match.
Nothing in `web_api` calls these yet. Review will.

`delete_file` keeps the original baseline, so a later restore writes that
text rather than an intermediate edit. If this session created the file,
delete removes it and drops the baseline row. The path leaves the session
diff. A created row whose file is already gone is omitted, so a drop that
did not land still shows no change.

Git is not read or written. Two sessions keep two baselines for the same
path. Restoring one does not know about the other.

## Rejected alternatives

- **A shadow git repository.** Revert would be `git checkout` of a snapshot.
  That couples the journal to git, touches files the session did not change
  when the snapshot is restored wholesale, and fails in a tree that is not a
  repository.
- **Storing each search-and-replace as the hunk.** Overlapping edits and a
  change made outside the tool do not compose back to the original bytes.
  The baseline is the original. Hunks are computed when something asks.
- **Replacing the baseline on every write.** The second edit would forget
  the bytes from before the session started, and review could not restore
  them.

## Failure modes

- A write that fails after the baseline row is inserted leaves the original
  bytes stored. The next successful write of that path keeps them.
- A non-UTF-8 file is refused before a baseline is stored, so a delete always
  has text to put back.
- `reject` returns `hunk no longer matches` when the current lines differ.
  It does not write.
- Deleting the chat session deletes its baseline rows.
