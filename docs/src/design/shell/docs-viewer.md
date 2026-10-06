# Docs viewer

This page is the workspace Markdown viewer: a file tree of the workspace's
markdown pages on the left and the rendered document on the right. It is the
read-only first slice of M10's project navigation. The editor and the docs
*mode* are later in M10.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| The `docs` agent mode (its tool set and prompt prefix) | M10 F10.1, `docs/src/roadmap.md` |
| Editing a page in the viewer | M10 F10.3. The viewer only reads; the chat tray is how the agent changes a page |
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
review route. In the window bar, at the right edge of the sidebar and above
the workspace menu, the sidebar holds back and forward, then a pair of
icons, **Chat** and **Documentation**. The pressed icon follows
the route. **Documentation** goes to `#/docs`; **Chat** goes back to the active
chat's route — `#/sessions/{id}`, or `#/` on the draft, so a chat reopened from
documentation keeps its session. A
hover names the icon. **Docs starts off**: a fresh launch lands on the chat route, and
a `#/docs` hash kept from the last launch is cleared before the router mounts, so
the viewer never reopens on its own. The window bar uses the same split as chat: sidebar fill
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
is `min-width: 640px` and `max-width: 880px`, pinned to the left of the pane.
The floor keeps tables and code blocks readable when the window is narrow; the
pane scrolls horizontally rather than crushing the text. Left rather than
centered: the chat tray overlays the right edge, so a left-aligned sheet is
occluded less while the tray is open. At wide sizes the sheet keeps its max
width and does not stretch to a hard-to-read line length.

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
waits for a click. Choosing another session, or **New chat**, keeps the viewer
open and shows that chat in the tray (below).

**Each open page is a history entry.** Selecting a file, a search hit, or a
document link sets `file` on `#/docs` and pushes a history entry. The back
and forward buttons in the window bar walk that history, and so do the side
mouse buttons: button 3 goes back, button 4 goes forward. On macOS the webview
never delivers those buttons, so the desktop process watches for them and
sends the same step. They walk through pages and
then through the routes that led here. The page remembered
from the last visit is written with replace, so returning to the viewer does
not add an extra step. Backing up to an entry with no `file` shows the empty
prompt again.

## Chat tray

The viewer and a chat share the screen. A tray on the right holds the currently
selected session's transcript and composer, so a page can be read and revised
with the agent in one place.

**The tray is an overlay, and its width is adjustable.** It is `position:
absolute` against the main column, pinned to the top, right, and bottom. It
opens at `min(420px, 42vw)`, clamped to 300–900 px and to a maximum that always
leaves 320 px of the page visible on the left. A drag handle runs down its left
edge: dragging left widens it and dragging right narrows it, the cursor over it
is `col-resize`, and the handle lights up in `--accent` while hovered or
dragged. The handle is focusable (`role="separator"`,
`aria-orientation="vertical"`), and **Left**/**Right** step the width by 24 px
while **Home** and **End** jump to the minimum and maximum. The width is
remembered in `localStorage` under `robi.docsTrayWidth`, so it survives a
reopen, and it is re-clamped when the window shrinks. It does not push the
document: the sheet keeps its measure and does not gain a horizontal
scrollbar. The window bar, the sidebar, and the docs search header are
unchanged, and the bar's title stays **Documentation**.

**It starts closed.** With no tray open, a slim handle sits on the right edge
of the main column (a **Show chat** button, `aria-expanded="false"`,
`aria-controls` the tray). Clicking it, choosing a session, or **New chat**,
opens the tray; the two later actions no longer leave `#/docs`, so the page
stays open and the tray shows the chat. The panel header carries the session
title and a **Close chat** button, and **Escape** closes it. Leaving `#/docs`
closes it, so the next visit starts closed again.

The tray body is the same chat surface as the main column — the transcript, the
mode and model controls, and the composer — with no pending-edit review strip.
A session that is still running shows the same activity line and **Stop**
control as it does in the main column.

**An agent edit refreshes the page.** The store already bumps a per-session
review tick when a tool call lands and when a turn finishes. The viewer reads
the active session's tick and re-fetches the open document and the tree listing
on each change. A fetch that returns the same text leaves the state object
alone, so a no-op tick does not reset the rendered document or its scroll
position. A page the agent created appears in the tree on the same tick.

### Attach a line to chat

A line of the open page can be sent to the agent without retyping it. Hovering a
rendered block — a paragraph, a heading, a list item, a table row, a code fence,
a blockquote, a rule — shows a small **add to chat** button (a chat bubble with
a plus) at the right of that block.
Clicking it adds the block's **raw markdown lines** to the composer as a file
attachment and opens the tray.

The unit is a rendered block, but the range is a source range.
`AssistantMarkdown`'s document mode stamps each block's element with
`data-md-lines`, taken from the mdast `position` react-markdown already carries
on every component. That is the line in the file the page was rendered from,
never a count of rendered elements or wrapped visual lines. The hover reads that
attribute (`target.closest('[data-md-lines]')`), so the innermost block under the
pointer wins: a paragraph inside a list item carries the paragraph's tighter
range, and a tight list item falls back to its own. A paragraph that wraps over
several source lines attaches the whole paragraph (`40-44`); a heading, a table
row, or a single-line paragraph attaches one line. The button is pinned to the
hovered block's top inside the sheet (`data-md-attach`, `data-find-ignore`), and
it clears on the next pointer move that is not over a block and on any scroll.

**The attachment is built client-side, like the paperclip's.** The viewer slices
the raw lines (`attachmentFromDocument`), sets the chip's `start_line` /
`end_line`, and hands the composer a ready `FileAttachment` with
`content_base64`. The document's workspace root becomes the `absolute_path`, so
the server stores the workspace-relative `path` and the model can re-read the
page with `read_file`. A slice over the 64 KiB per-file cap is refused with a
notice in the viewer rather than silently dropped. See
[file-attachments.md](file-attachments.md).

**A request outlives the closed tray.** Attaching a line opens the tray
(`setTrayOpen(true)`), which mounts a composer that was not there a moment ago,
and the request must survive that gap. So `AppLayout` does not pass the
attachment down as a prop: it calls `requestComposerAttachment(draftKey, file)`,
a keyed one-shot queue (`src/state/composerAttachments.ts`), and the composer
drains it on mount and on each request. The key is the composer's `draftKey` —
the selected session id, or `draft` when no session is selected, so a line
attached with no chat open lands in a new chat and the first send creates the
row. The drained attachment goes through the same count and total-size caps as a
picker or a drop (`appendAttachments`); anything dropped sets the composer's
error, and the field takes focus so the user can write about the line.

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
ranked slot. The surviving chunks are then grouped by document: one row per
document, its `score` is the number of matching chunks, and the best-scoring
chunk supplies `start_line`, `end_line`, `title`, and `snippet`. A document
that matches many times outranks one that matches once; ties keep the fusion
order.

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
| `indexing` | `Indexing files_done/files_total — results may be incomplete`, or `Preparing search…` before the walk has seen a file |
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

## Find in the document

The header's **Search documentation…** field searches across files. A second,
separate search finds text inside the document that is open on the right. It is
opened with **Cmd+F** on macOS and **Ctrl+F** elsewhere — the platform is read
from `navigator.platform`, falling back to the user agent — and closes on
**Escape** or the bar's close button.

**Scope is the open document.** The search walks the text nodes under the
viewer element only: the tree, the cross-file results pane, and the header are
not searched. The shortcut is intercepted only while a document is rendered
(`selected` and `content` both set), so Cmd+F keeps its default behaviour
everywhere else in the app.

**The bar.** A text field (a literal substring, not a pattern), a match count,
previous and next buttons, a **Match case** toggle (`aria-pressed`), and a
close button. The count reads `N of M` one-based, says **No results** for a
query with no matches, and shows nothing for an empty field. Enter steps
forward and Shift+Enter steps back while the field has focus. The step buttons
wrap: next from the last match goes to the first, and previous from the first
goes to the last. They are disabled when there are no matches. Every match is
painted; the active one is scrolled to the middle of the pane.

**Matching is per text node.** A query is matched against each text node in
turn, so a phrase broken by an inline element — a word wrapped in emphasis, say
— is two text nodes and does not match as one hit. Case-insensitive by default;
the toggle makes it case-sensitive. The offsets are kept in original-string
coordinates by escaping the query and walking a `gi`/`gu` regular expression, so
case folding never shifts a match.

**Highlighting does not touch the DOM.** Matches are held as `Range` objects and
registered with the **CSS Custom Highlight API** (`CSS.highlights`), styled by
`::highlight(robi-doc-find)` and `::highlight(robi-doc-find-current)` in the
screen's stylesheet. The React-managed document is never mutated, so a re-render
of `AssistantMarkdown` — which happens on every screen render, because
`onDocLink` is a new function each time — cannot corrupt the search state. The
active-match registry entry is set after the all-matches one, so it paints on
top.

The API is feature-detected (`typeof Highlight === 'function'` and
`'highlights' in CSS`). It ships in Chrome/Edge 105+, Safari 17.2+ (the macOS
WKWebView tracks Safari), and Firefox 140+. Where it is missing — a pre-17.2
WKWebView, or the `happy-dom` test environment — the bar still counts, steps,
and scrolls; only the color highlight is absent.

**Skipped regions.** The walk ignores `script`, `style`, `svg`, and
`[aria-hidden="true"]` subtrees, plus `[data-find-ignore]`. Diagram SVG text is
therefore out, and `MermaidDiagram` marks its visually-hidden copy of the source
with `data-find-ignore` so that copy is out too; the visible fence before a
diagram resolves stays searchable.

**Late DOM changes re-run the search.** A `mermaid` fence that resolves into an
SVG changes the text under the viewer after the first pass. While the bar is
open, a `MutationObserver` on the viewer (child list, subtree, and character
data) re-runs the search, debounced to one pass per animation frame, so the
ranges and the count stay correct.

### Rejected alternatives

- **Wrapping matches in `<mark>`.** Injecting nodes fights React's
  reconciliation: `AssistantMarkdown` re-renders on every parent render, so the
  injected nodes would be clobbered or would break the tree.
- **A rehype plugin that rewrites the tree before render.** It cannot report a
  total count and a current index back to the bar cleanly, and it still cannot
  match across inline nodes.
- **A search box that also filters the tree.** The tree is a different scope.
  Keeping find to the open document matches the reader's mental model and the
  browser's own Cmd+F.

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
- **Injecting the attach button into each rendered block.** It fights the same
  React reconciliation the find feature avoids (the viewer never mutates the
  rendered DOM), it is clipped by a `pre` or a diagram's `overflow`, and it
  cannot sit on a table row. One overlay button positioned from the hovered
  block is simpler.
- **Mapping the hover to a single visual line.** Turning a wrapped line back into
  a source line needs character-offset reconciliation through markdown rendering,
  and inline emphasis and entity rewriting shift those offsets. The block's
  source range is exact and always a coherent slice.

## Failure modes

- A scan error is not fatal: an entry that cannot be read is skipped, not the
  request.
- A failed listing shows the error in place of the tree. A workspace with no
  markdown shows **No markdown files in this workspace.**
- A failed content fetch shows the error in the viewer. The previous document
  stays on screen while the next one loads.
