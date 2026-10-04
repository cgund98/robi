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

The selected document renders with the same `AssistantMarkdown` component as
assistant text, in its `document` mode: GFM, headings stepped down by level,
tables, and fenced `mermaid` diagrams. No document opens on page load: the
viewer starts with a centered **Select a document to open it.**, and the tree
waits for a click. Choosing another session, or **New chat**, leaves the
viewer for the chat.

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
