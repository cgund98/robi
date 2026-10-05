# Docs viewer

This page is the workspace Markdown viewer: a file tree of the workspace's
markdown pages on the left and the rendered document on the right. It is the
read-only first slice of M10's project navigation. The editor and the docs
*mode* are later in M10.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| The `docs` agent mode (its tool set and prompt prefix) | M10 F10.1, `docs/src/roadmap.md` |
| Editing a page, and the agent editing the same file | M10 F10.3. This page only reads |
| Widening the filter past markdown and the docs directories | M10 F10.2. This page shows markdown only |
| The shared path filter and grants | [roadmap](../../roadmap.md) (M3) |
| Markdown rendering itself (GFM, mermaid, headings) | [chat-ui.md](chat-ui.md) |

## Problem

The workspace already holds its own documentation — `README.md`, `AGENTS.md`,
`docs/`. Reading it means opening an editor outside the app. The review screen
already shows a two-column tree-plus-body layout for a set of changed files;
documentation wants the same shape over a different file set.

## Decision

**Two workspace routes, one screen.** `GET /api/v1/workspaces/{id}/docs` lists
the markdown files; `GET /api/v1/workspaces/{id}/docs/{path}` returns one
file's text. The shell opens `#/docs`, which keeps the session sidebar like the
review route. The window bar uses the same split as chat: sidebar fill
over the sidebar column, canvas over the rest, so the sidebar color
reaches the window buttons.

**The scan is a gitignore walk.** The listing uses the same
`ignore::WalkBuilder` defaults as `grep`: hidden entries are skipped, and
`.gitignore`, `.git/info/exclude`, and parent ignore files are respected.
`node_modules/`, `target/`, and `dist/` therefore stay out without a special
case. Symlinks are not followed. A file counts when its extension is `md` or
`markdown`, case-insensitively. The walk stops at 500 files. Paths come back
workspace-relative with `/` separators, sorted.

The listing is paths only. The client fetches content per selection, so opening
one page does not pull the whole set over the wire.

**The content route confines itself.** The handler joins the relative path
against the canonical root, then canonicalizes and checks the result is still
inside the root and is a file. A path that escapes is `404`, the same as a
missing file. A non-markdown name is `400`. A file that is not UTF-8 is `400`,
matching the review route. The text is capped at 512 KiB; a larger file ends
with `[The tail of this file was cut.]`.

**The tree is cached per workspace.** The listing is remembered by workspace
id across visits. The first visit for a workspace shows the loading state; a
later visit paints the cached tree straight away and refreshes in the
background, so the tree appears instantly and only changes when the new
listing arrives. A failed refresh keeps the cached tree rather than replacing
it with an error.

**The tree is collapsible.** The left pane is a tree of markdown paths built
from `buildFileTree`, so only directories that contain a page appear.
Directories sort before files, each group alphabetical and case-insensitive.
A directory row is a button that folds its children; the default is expanded.
The open state is held by the screen, not the tree, so fetching the next
document does not fold the tree. A file row selects that path and highlights
while it is open.

**The document has a measure.** The right pane scrolls, and the sheet inside it
is `min-width: 640px` and `max-width: 880px`, centered. The floor keeps tables
and code blocks readable when the window is narrow; the pane scrolls
horizontally rather than crushing the text. At wide sizes the sheet stays
centered and does not stretch to a hard-to-read line length.

The pane is the only scroller on that side of the screen. It is
`position: relative`, so the hidden source label on a diagram stays inside
it and does not extend the window. The screen title stays in the window
bar, which sits above this column, so this column does not clip it.

The selected document renders with the same `AssistantMarkdown` component as
assistant text, in its `document` mode: GFM, headings stepped down by level,
tables, and fenced `mermaid` diagrams. A relative link whose path ends in
`.md` or `.markdown` resolves against the open file and selects that page in
the viewer. A link that would leave the workspace stays a link. Absolute
URLs and in-page fragments still open as links. No document opens on page load: the
viewer starts with a centered **Select a document to open it.**, and the tree
waits for a click. Choosing another session, or **New chat**, leaves the
viewer for the chat.

**Each open page is a history entry.** Selecting a file, a search hit, or a
document link sets `file` on `#/docs` and pushes a history entry. The side
mouse buttons walk that history: button 3 goes back, button 4 goes forward,
through pages and then through the routes that led here. The page remembered
from the last visit is written with replace, so returning to the viewer does
not add an extra step. Backing up to an entry with no `file` shows the empty
prompt again.

## Search

A query field sits in the screen header, with a
**Semantic** / **Text** control. The search starts half a second after the
last keystroke. Another keystroke, or a change of engine, clears that wait
and aborts a request already in flight, so only one search is open and only
the latest text is sent. Switching engines searches the current text
immediately. While that pause or the request is still open, a spinner sits
in the field and the results pane says **Searching…**. Ranked hits then
replace the tree; the viewer on the right still shows whichever document is
open, and clicking a hit opens it. Clearing the field brings the tree back.

**One route, two engines.** `GET
/api/v1/workspaces/{id}/docs/search?q={query}&limit={n}&engine={engine}`
picks the engine. `engine` defaults to `semantic`. `ripgrep` is the Text
control. Anything else is `400`. `limit` defaults to 10 and may not exceed
20; a blank or missing `q`, or a `limit` outside 1 to 20, is `400`.

**Semantic** runs the fused vector-plus-FTS query described in
[semantic-search.md](../intelligence/semantic-search.md) and keeps only
markdown hits. The filter runs after fusion, so a code hit cannot hold a
ranked slot.

**Text** is a case-insensitive literal scan of markdown. It runs `rg` when
that binary is on `PATH` (`--fixed-strings`, `--ignore-case`, markdown globs, ripgrep's own
hidden and gitignore defaults). If `rg` is missing or fails to launch, the
handler scans the same markdown set the tree lists. It does not start the
index. Each file appears once, at its first matching line, and files are
ordered by how many lines matched. `start_line` and `end_line` are that
line, `title` is the file name, `snippet` is the line cut at 500 bytes,
and `score` is the match count. The response still includes `engine` and
`index`, and the screen does not treat `index` as a partial-result notice
for this engine.

**The response says whether it is complete.** Each response carries the
index status its hits were taken from:

```json
{
  "query": "how do sessions pause",
  "engine": "semantic",
  "index": { "state": "ready", "files_done": 120, "files_total": 120, "error": null },
  "hits": [
    {
      "path": "docs/src/design/core/agent-loop.md",
      "start_line": 40,
      "end_line": 88,
      "title": "The paused turn",
      "snippet": "The invariant the whole design rests on…",
      "score": 0.0322
    }
  ]
}
```

For a semantic hit, `title` is the section's heading chain (`Install.Overview`),
`snippet` is the chunk's opening cut at 500 bytes on a character boundary, and
`score` is the fusion score. A state other than `ready` means the corpus was
incomplete, so the hits may be too, and the screen says so above the results.
That notice is only for the semantic engine:

| `state` | Notice |
|---|---|
| `indexing` | Indexing `files_done`/`files_total` — results may be incomplete |
| `downloading` | Preparing search… |
| `paused` | Search index paused, with **Resume** |
| `failed` | Search index failed, with **Resume** |

**The first query starts the index.** The request holds a lease, so the scan
begins if no chat session has one, and the hub keeps the task for a 60-second
linger after the request ends. Until the scan catches up the state is
`indexing`, the hits are partial, and that is the honest answer rather than
an error.

**While a notice is up, the screen polls.** The event stream's index frames
follow the session's event stream, which a docs visit does not open, so the
screen `GET`s `/api/v1/workspaces/{id}/index` every 2 seconds while the state
is not `ready` and re-runs the search when the state changes. That GET shares
the shell's 10-second cache, so a tick inside that window returns the last
status and the counts move on when the cache expires.
`paused` and `failed` wait for **Resume** instead: it calls
`PUT /api/v1/workspaces/{id}/index` with `running`, and the next poll picks
the scan up. `ready` draws no notice and stops the poll.

## Rejected alternatives

- **One route returning every page's content.** A docs set is opened one page
  at a time; shipping all of it makes the first paint wait on the largest file.
- **A new markdown parser or renderer.** `AssistantMarkdown` already renders
  GFM and mermaid; a second renderer would drift.
- **A separate tree type.** `buildFileTree` is already a generic path-to-tree
  helper. The docs tree reuses it and only forks the row component, because its
  rows toggle instead of sitting still.
- **Persisting the collapsed set.** The default-open tree is small after the
  ignore filter; a stored fold state is a setting nobody asked for.

## Failure modes

- A scan error is not fatal: an entry that cannot be read is skipped, not the
  request.
- A failed listing shows the error in place of the tree. A workspace with no
  markdown shows **No markdown files in this workspace.**
- A failed content fetch shows the error in the viewer. The previous document
  stays on screen while the next one loads.
