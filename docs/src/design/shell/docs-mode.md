# Docs editing

This page is the editing half of the [docs viewer](docs-viewer.md): CodeMirror
over the open page, debounced autosave, and the reconciliation that keeps a
manual edit and an agent edit from losing each other. It is **F10.2** in the
[roadmap](../../roadmap.md).

"Docs mode" here means the documentation **view** — the `#/docs` route and its
toolbar. It is not an agent mode, and no `docs` agent mode is planned.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| The read-only tree, search, and find bar | [docs-viewer.md](docs-viewer.md) |
| The three edit tools and their approval cards | [editing-tools.md](../tools/editing-tools.md) |
| The session baseline a save records | [checkpoints.md](../tools/checkpoints.md) |
| The SSE envelope and reconnect | [events-sse.md](events-sse.md) |
| Markdown rendering (GFM, mermaid) | [chat-ui.md](chat-ui.md) |

## Problem

The viewer reads a page; a change means leaving the app. The obvious save is a
whole-buffer `PUT`, but the agent edits the same files, and a whole-buffer write
silently discards an agent edit that landed while the reader was typing. Two
copies of the file cannot be the answer either: the agent reads the disk.

## Decision

**The editor sends deltas, and the server reconciles.** The open page mounts a
CodeMirror 6 `EditorView`. Each transaction's `ChangeSet` is accumulated. One
second after the last keystroke the accumulated set goes to `PUT
/api/v1/workspaces/{id}/docs/{path}`. The server applies it to the base version
the client names; if the disk has moved on, a three-way merge folds the two
together.

**A version is a content hash.** `GET .../docs/{path}` returns `version`, the
`sha256:` hash of the bytes it just returned, and stores those bytes in a
bounded per-process cache. A save names the version it was built from. The cache
is not session state and not per client: the request carries everything needed
except the base text, and the cache is only how the server recovers that.

**Only the base text has to be remembered.** A client that never fetched a page
has no version to name, and a base older than the cache is gone. Either is a
`409` carrying the current `{content, version}`; the client re-sends the whole
buffer against the new version, and the user's text wins outright.

**Overlapping hunks go to the client; disjoint hunks both apply.** `merge3`
diffs the base against the disk and against the client's target, then walks the
hunks in order. A hunk that does not touch the other side applies. Where they
overlap the client wins, because the save is the later edit. An insert that only
touches the boundary of a replaced region composes rather than conflicts.

**The save is the M4 write path.** Confinement, the per-path lock, the baseline,
and the atomic write are the same helpers the edit tools use, so a manual edit
gets a checkpoint like an agent edit. The baseline is per path per session and
the first write wins, so a 1-second autosave cadence does not mint a checkpoint
per keystroke burst — the session's revert point stays the bytes from before the
first save. A save is a user action and does not go through the approval card.

**A write announces itself, whoever wrote it.** Every successful save and every
edit-tool write publishes `robi.workspace.v1.file_changed`, subject the workspace
id, with `source` (`user` or `agent`), `path`, `session_id`, and `outcome`
(`applied`, `merged`, `created`, `deleted`, or `unchanged`). The viewer refreshes
the tree on any frame. With autosave there is no Save click, so this frame and
the save's own status line are how the reader learns an update went through.

### Autosave

State per open document:

- `base` — the version the pending changes were built from.
- `pending` — the `ChangeSet` accumulated since `base`.
- `full` — send the whole buffer next (a create, or after a `409`).
- `inflight` — one request at a time.

Rules:

1. On a user change, compose it into `pending` and re-arm the 1000 ms timer.
2. On the timer, or on a flush, send. If a request is already open, do nothing;
   the response re-arms when `pending` is non-empty.
3. On `200`, adopt the response's version. Input that arrived during the flight
   re-accumulated and goes out on the next debounce. An `outcome` of `merged`
   replaces the buffer with the server's text.
4. On `409`, adopt the response's version and re-send the whole buffer.
5. On an error, keep `pending`, show it, and retry — one debounce, then doubling
   to an 8-second cap.

**Flush points beyond the timer.** Cmd/Ctrl+S, leaving Edit mode, choosing
another document, the window losing focus, and unmount. The flush ignores the
debounce.

**The buffer is not clobbered.** The viewer already refetches the open page on a
review tick. It also refetches on a `file_changed` frame, but skips the open
document while `pending` is non-empty and shows **Agent changed this file —
saving will merge.**; the next autosave merges. When the buffer is clean the
page is adopted in place, and only while the editor is unfocused, so a remote
change does not move the caret. A clean buffer always adopts the new base
version.

### Toolbar

A bar sits at the top of the document pane with a **Preview | Edit** segmented
control — an **eye** then **Preview**, a **code bracket** then **Edit** — a
**Save** button (a manual flush), a dirty dot while `pending` is non-empty, and
a status line: **Saving…**, **Saved**, **Saved — merged with an agent edit**,
**Out of sync — re-sending**, or the error. The choice is remembered per
workspace.

The editor is created once against a ref when Edit is enabled, and destroyed when
the mode or the document changes — never rebuilt on render. Its theme **and its
syntax highlighting** are built from the shell tokens; there is no second
palette. Headings, emphasis, links, inline code, quotes, rules, and list markers
are colored, so the source and the Preview read as the same document. The editor
draws its own cursor (`drawSelection`) rather than leaning on the browser's
native caret, which the production WebKit webview does not render reliably; the
theme's accent color reaches both the native `caret-color` and the `.cm-cursor`.

**Edit can open at a line.** A hovered block's **Open in editor** action flips
the mode and asks the editor to `revealLine`, which puts the cursor at the end of
that source line, scrolls it into view, and focuses the view. The cursor move is
not an undo step. The request is queued on the screen and applied by the mount
effect, because the editor does not exist yet at the moment the mode changes. See
[docs-viewer.md](docs-viewer.md#hover-actions-on-a-block).

## Rejected alternatives

- **A whole-buffer `PUT`.** Simple, and it discards an agent edit that landed
  during the debounce window. The delta plus merge keeps both.
- **A per-connection buffer the server owns.** Buffers would need eviction,
  reconnection handling, and a rule for two windows on one page. A content hash
  is stateless and survives a reload.
- **Line-based deltas.** CodeMirror works in character offsets; converting to and
  from lines costs more than the merge saves.
- **Applying a delta to the disk text instead of the cached base.** The offsets
  are the base's, so the delta would land in the wrong place. The cache is what
  makes a delta meaningful.
- **A merge conflict the user resolves.** Reserved for a gone base version. The
  common case is one agent hunk and one user hunk, which compose.
- **A WYSIWYG editor.** It needs a faithful DOM-to-markdown serializer, and it
  fights the viewer's rule that the rendered DOM is never mutated. A
  source-buffer editor with the existing renderer beside it keeps one source of
  truth.
- **Blocking the save on a clean buffer.** Autosave has no Save click; refusing
  to write because the user is "still typing" leaves the file stale.

## Failure modes

- A `409` recovers with one full-buffer resend. Another `409` repeats it.
- A save error keeps the buffer and the pending set; a status line reports it and
  the retry backs off. Closing the view flushes what it can.
- A `file_changed` frame for a deleted page leaves the tree without the path; the
  next selection is a new document.
- Saving with no open chat session writes without a baseline. The setting is not
  required to edit.
