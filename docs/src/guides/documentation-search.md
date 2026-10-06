# Documentation viewing and search

Robi can read the documentation already in your workspace — `README.md`,
`AGENTS.md`, `docs/`, and any other markdown — inside the app, and search it
with two engines. The viewer is read-only; it is a development slice of the
planned project navigation.

## Open the viewer

Choose the **Documentation** icon in the window bar, above the workspace menu, or go to `#/docs`. The
session sidebar stays in place and the main column becomes the viewer. The
viewer starts closed: the app opens on the chat, and a docs page left open
from a previous session does not reopen on launch.

The viewer has two panes:

- **Left — the page tree.** Every markdown file in the workspace, built from the
  same gitignore walk as `grep`: hidden entries and `.gitignore`d paths are
  skipped, so `node_modules/`, `target/`, and `dist/` stay out. Symlinks are not
  followed, and the walk stops at 500 files. Directories sort before files, each
  group alphabetical and case-insensitive, and every directory folds and expands
  (expanded by default).
- **Right — the document.** Nothing opens on load; the viewer starts with
  **Select a document to open it.** Click a page in the tree to render it. The
  document renders with the same markdown component as assistant text: GFM,
  stepped-down headings, tables, and fenced `mermaid` diagrams, in a column
  (640–880px) at the left of the pane, so wide tables and code stay readable
  and the chat tray occludes less of the page.

The tree is cached per workspace, so a return visit paints it instantly and
refreshes in the background. The page you last opened, the folders you folded,
and each page's scroll position are remembered too.

**Links work.** A relative link ending in `.md` or `.markdown` resolves against
the open file and opens that page in the viewer. A link that would leave the
workspace stays a plain link, and absolute URLs and in-page fragments open as
usual. Each page you open is a history entry, so the side mouse buttons walk back
and forward through pages.

## Chat beside the page

You can read a page and revise it with the agent without leaving the viewer. A
tray on the right holds the current session's conversation and its message box.

- It starts closed. A handle on the right edge of the window — **Show chat** —
  opens it. Choosing a session, or **New chat**, while the viewer is open also
  opens it, and keeps you on the documentation page.
- The tray header names the session and has a close button; **Escape** closes
  the tray too. Leaving the viewer closes it, so it is closed again next time.
- The tray is the same chat you get in the main window: the transcript, the mode
  and model controls, and the message box. It overlays the right of the page
  rather than pushing it, so the document keeps its width. Drag its left edge to
  make it wider or narrower — the width is remembered next time.
- Ask the agent to change the page you have open, or another page, and the
  viewer refreshes as the edit lands. A new page the agent creates shows up in
  the tree. You do not have to reopen anything.

## Send a line to the chat, or edit it

You can point the agent at a specific part of the page without retyping it, or
jump straight to that part in the editor. Hover a block — a paragraph, a
heading, a list item, a table row, a code fence — and an **ellipsis** button
appears at the right of that block. Click it for two choices:

- **Add to chat** adds the block to the message box.
- **Open in editor** switches to the editor and puts the cursor at the end of
  that block's first source line.

The block is added to the message box as an attachment chip, the chat tray opens
if it was closed, and the caret goes to the message box so you can say what you
want done. The attachment is the block's **raw markdown lines** — what the file
actually says, including a table's pipes or a fence's backticks — so the agent
reads the source and can widen the read if it needs to. A paragraph that wraps
over several lines attaches the whole paragraph; the chip shows the range, like
`docs-viewer.md (40-44)`.

If no chat is open the line goes to a new chat, and sending starts it. The usual
attachment limits apply, and a block too large to attach tells you why instead of
being added.

## Search

A **Search documentation…** field sits in the header, with an engine toggle:
**Semantic** or **Text**. The search starts half a second after your last
keystroke. Another keystroke — or a change of engine — cancels the wait and
aborts any request already in flight, so only one search runs and only the latest
text is sent. While it waits or runs, a spinner sits in the field and the results
pane says **Searching…**. A field with no text brings the tree back.

Results replace the tree on the left; the document on the right stays open.
Click a hit to open it. Clearing the field restores the tree.

### Semantic

The default. It runs the fused vector-plus-full-text index and keeps only
markdown hits, so a code hit cannot take a ranked slot. A hit's title is the
section's heading chain (for example `Install.Overview`) and its snippet is the
start of that chunk.

**The first semantic query starts the index.** Until the scan finishes the
results are partial, and the screen says so above the hits rather than failing:

| Notice | Meaning |
|---|---|
| **Indexing N/M — results may be incomplete** | the scan is still running |
| **Preparing search…** | the index is being downloaded |
| **Search index paused.** | with a **Resume** button |
| **Search index failed.** | with a **Resume** button |

While a notice is up the screen polls the index status every two seconds and
re-runs the search when the state changes, so the hits fill in as the scan
catches up. **Resume** restarts a paused or failed scan.

### Text

The **Text** control runs a literal, case-insensitive scan of markdown via
`ripgrep`. If `rg` is not installed it scans the same markdown set the tree
lists. It does not start the index and returns immediately. Each file appears
once, at its first matching line, ordered by match count.

## Find in the open document

The header field searches across files. To search the document already open on
the right, press **Cmd+F** (macOS) or **Ctrl+F** (Windows and Linux). A small bar
appears over the top-right of the document:

- **The field** matches the text you type literally, not as a pattern. Matching
  ignores letter case.
- **The count** shows your position — `1 of 4` — or **No results** when nothing
  matches. An empty field shows nothing.
- **The up and down arrows** step to the previous and next match. They wrap
  around: down from the last match goes to the first. Inside the field, Enter
  steps forward and Shift+Enter steps back. The arrows are greyed out when there
  is nothing to step through.
- **The `Aa` button** turns on **match case**, so `Rust` stops matching `rust`.
- **The `×`**, or **Escape**, closes the bar.

Every match is tinted, and the one you are on is tinted brighter and scrolled
to the middle of the pane. The search reads the text as rendered, so a phrase
split by formatting — say, a word in **bold** in the middle — is not found as
one hit; search for a piece that stays together. Diagram pictures are skipped:
search finds the `mermaid` source while the fence is on screen, not the drawing
it becomes.

Find searches only the open document. To search every file at once, use the
header field described above.

## Limits

- A scanned file that cannot be read is skipped, not the whole request.
- A file over 512 KiB is cut, ending with `[The tail of this file was cut.]`.
- A non-UTF-8 or non-markdown file is rejected.
- A workspace with no markdown shows **No markdown files in this workspace.**

## Where this is specified

The two routes, the engine choice, the index states, and the history behaviour
are in [Docs viewer](../design/shell/docs-viewer.md). The ranking behind the
semantic engine is in [Semantic search](../design/intelligence/semantic-search.md).

## Next

- [Tools](../reference/tools.md) — how the assistant searches the same corpus.
- [HTTP API](../reference/http-api.md) — the docs and search routes.
