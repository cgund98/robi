# Chat UI

This page is how the shell shows a chat with no tools. It covers the draft
session, the transcript the window stores, the activity line, and the composer
lock. Look and tokens stay in [visual-style.md](visual-style.md). Event
delivery stays in [events-sse.md](events-sse.md).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Streaming caret and painting `message_delta` text | Later on this page. The assistant row is stored only when the model stream finishes, so this cut does not paint tokens |
| Syntax highlighting, copy, retry, edit-and-resend | Later on this page. Assistant text is Markdown; highlighting is not |
| Mermaid diagrams | This page. A fenced `mermaid` block renders as a diagram in assistant text, on a plan page, and in the markdown file preview |
| Grant and session-allow editing | `docs/src/design/workspace/permissions.md` (M3) |
| Creating the session row | [persistence.md](../persistence/persistence.md). The shell delays that call |
| `/id` skill mentions and the **Using** row | [skills.md](../reach/skills.md) |
| A shell card showing the uncompressed command output | [shell-output.md](../compression/shell-output.md). The transcript body the model sees may be the compressed view |
| An MCP card showing the bounded server text | [mcp-output.md](../compression/mcp-output.md). The transcript body the model sees may be the compressed view |

## Draft session

**New session** does not `POST /chat_sessions`. It selects a client-only draft:
no database row, no sidebar entry, header title “New session”. Choosing it
again while it is already open does nothing. While that draft is selected,
**New chat** uses the same active pill as a recent session.

The first send creates the session, then `POST /chat_sessions/{id}/messages`
with `{ "instruction" }`. On `202` the shell selects that session, marks it
thinking, and keeps a local echo of the user text until a fetched user message
with that text is new in the list. If create succeeds and submit fails, the
session stays and the draft text stays in the composer so the retry is only
the message post.

App load lists sessions and selects the most recent. An empty list opens the
draft.

**Each chat has its own route.** A session is `#/sessions/{id}`; the draft is
the index route. Choosing a session pushes its route, so the window's back and
forward controls — and the side mouse buttons — walk the chats in the order
they were opened. A forward entry is dropped once a new chat is pushed. Back or
forward selects whatever session the route names. App load opens the most
recent session on its own route; the draft route stays the draft. The route is
restored on reload, so a reload returns to the same chat.

The window bar shows “New chat” on the draft and the session title once a row exists. MCP server marks sit on the right of that line in both cases. To their right, a mark appears when `GET /api/v1/health` fails or does not answer within 3 seconds. Hovering the mark says whether the check timed out, which HTTP status came back, or the error message. The shell checks every 5 seconds and removes the mark after a successful check. The first click on a session that has not been opened in this window shows a spinner and “Loading conversation” until its transcript loads. A later click on the same session does not. Until a prompt is submitted, a greeting
sits in the center — **Good morning**, **Good afternoon**, or **Good evening**,
from the local hour — with the composer in a card under it. The first echo or
stored message returns the title, the transcript, and the bottom composer.

The pencil on a session opens a rename dialog with the stored title. The dialog
is a Radix dialog. An unset title starts the field empty. Save sends `PATCH`
with the trimmed title. Cancel and Escape leave the title as it is. An empty
title is refused in the dialog and is not sent.

The `×` on a session opens a delete confirm dialog, also a Radix dialog, in
place of a native `window.confirm`. It names the session and says the delete
cannot be undone. When that session's agent is running it adds a muted line,
**A running turn will be stopped.** **Delete** sends `DELETE`; the server stops
the running actor first, so the request can take a moment. While that request
is in flight, **Delete** carries a spinner and both buttons are disabled, and
Escape and an outside click do not close the dialog. A failure shows the error
line and keeps the dialog open. Success closes it and the session leaves the
list.

## Transcript

Zustand holds the session list, the active id, messages keyed by session, and
a per-session phase: `idle`, `thinking`, or `responding`.

The list keeps the latest row in view while the reader is at the bottom.
Scrolling up, including a trackpad or wheel gesture upward, releases that
follow so a message update does not pull the view back down. Returning to
the bottom resumes it. Switching chats starts at the bottom again. A repeated
`message_delta` that does not change the phase does not render the list again.

The transcript is HTTP, not the event stream.

| Trigger | Request |
|---|---|
| Select a session, or the event stream opens | `GET /chat_sessions` and `GET /chat_sessions/{id}/messages`. The two transcript reads share one request while one is in flight |
| `message_added`, `message_updated`, `tool_call_updated` | `GET /chat_sessions/{id}/messages/{message_id}`, then upsert that row. One GET is in flight per message, and a newer frame schedules one more. A list reload keeps a row upserted after the list started |
| `turn_finished` | The session and the message list again |
| `robi.session.v1.created`, `robi.session.v1.updated` | That session again, unless the frame already carried the new title. A fetch that started earlier keeps a local title and a session inserted after it started. The message list is left as it is |
| `robi.session.v1.deleted` | Drop that session from the list |
| `robi.app.v1.error` | Record `message` and show it at the top of the shell until it is dismissed |
| Phase is `thinking` or `responding`, and no frame for that session has arrived for 2 seconds | The session and the message list again |

`message_delta` does not change message text. `kind: "reasoning"` sets
**Thinking**. `kind: "text"` sets **Responding**. Other delta kinds are
ignored. `turn_started` sets **Thinking**. `turn_finished` sets `idle`, then
refetches the session and the message list without taking the phase from
`has_pending_agent`. The actor clears that flag after the event. A
`turn_started` during the refetch stays **Thinking**. A reload while a turn
is already running still restores the phase from `has_pending_agent`. A failed turn, and every other shell error, is kept in memory and listed
under Settings → Audit log until Robi restarts. That page also lists the last 250 API calls, newest first, 50 per page, with method, OpenAPI path, status, and duration. Clicking a row shows that response body. `GET /api/v1/health` is left out. An API response outside 2xx, and a request that throws, is written to the error list too,
without a second notice. A fetch this page aborted, including the 10-second
timeout, is not. Several can be open at once. Each one sits under the window title and names
the chat when it has one. Dismiss hides it. The audit log keeps it. The 2-second refetch is what paints a stored turn when the frame that
would have loaded it was dropped. A frame for the session on screen resets that wait. A frame for another session does not.

Opening the stream refetches even on the first connect. A frame published
before the socket existed is recovered from the store. Reconnect does not
replay deltas.

## Tool calls

An assistant message renders its text as Markdown, then one row per tool
call. Headings, lists, tables, links, and fenced code are elements. Syntax
highlighting is not applied. A `tool` message is not shown again; the result
lives on the call. User text stays plain. The bottom of a finished turn is one row: **Worked for Ns** on the left, and a copy
icon on the right when that turn has assistant text. Hovering it says **Copy markdown**.
After a click the tooltip says **Copied**. The button copies that message's
Markdown as stored. A finished turn ends with a muted line, **Worked for
Ns**, measured from the user message to the last message in that turn.
A turn that is still running, or waiting on approval, does not show it.

Tool rows from later iterations of the same turn sit in that same stack, with
no extra gap between quiet rows. An edit card has a little space above and
below it. A shell card has a little space above and below it, so two panels do not touch. A finished read is a quiet line: an icon, a verb (`Read`, `Grepped`, `Found`,
`Listed`), and the path or pattern. A `read_file` call that names `offset` and
`limit` adds the inclusive window after the path, in `--ink-faint`, as `L240-299`.
`limit` alone is `L1-N`. `offset` alone is `L240`. A call that reads the whole file
shows no window. A finished `write_file`, `edit_file`, or
`delete_file` is a bordered card: the path the tool was called with
(`scratch/test.md`, `../gopi/test.md`, `/tmp/test.md`), the `+` / `−` counts beside
it, and 4 diff lines starting at the first added or removed line. Context above that change stays out of the closed card. Added and removed lines carry a left accent in
`--diff-add` or `--diff-del`. Clicking the name opens the rest, at most 24
lines, then `N more lines` when the change is longer. A running call shows a spinner.
A failed call shows the verb and target in `--danger`. Clicking a row that has a result or an error opens
the body: numbered file text, match lines, paths, or the error. The row stays
closed until that click. Every opened body is a bordered panel, the same shape as a shell card: the header sits
on the panel, and a rule divides it from the body — numbered file text, match lines, paths, or the error.

A finished `write_plan` is a bordered card. The label is **Created Plan**, or
**Updated Plan** when the result status is `updated`. The title is the first
heading in that call's markdown, otherwise `plan_name`, otherwise the file
name. Under it is the first paragraph of the markdown, clamped to three lines.
**View Plan** and **Build** sit at the bottom right, both in `--mode-plan`. **View Plan** replaces
the chat with a page that fills the main column. The header is **Back**, the
plan name, and **Build**. **Back**, or choosing another session, returns to
the chat. Opened from the docs chat tray it fills the main column over the
document sheet instead, and **Back** returns to that sheet; see
[docs-viewer.md](docs-viewer.md). The body lists that call's todo steps, then the markdown, rendered
the same way as assistant text. A step is pending, in progress, completed, or
canceled. A step with no content is left off. Opening the page again shows
the same markdown and the same steps. Either **Build** switches the session
to agent mode and sends
`Implement the plan at <path>. Read that file and make the changes it
describes.`, where `<path>` is the file that call wrote. **Build** stays
disabled while the composer is locked, and when the call has no path. A
running plan write shows a spinner and no card. A failed one stays the error
row.

A finished `todos` call is not a tool row. Each task that call marked
completed is a muted line in the transcript, in the order of the result list,
with the task text. A task it marked canceled is the same line, struck
through. A call that only moves a task to pending or in progress adds no
line. Under the conversation, above the activity line, the latest checklist
renders the tasks that are still pending or in progress once the session is
in agent mode. Each open task is one line, and a longer task is clipped. Plan mode leaves that list off, and the steps stay on the plan
page. The list comes from
the last successful `todos` result, or from the last successful `write_plan`
when no later `todos` call has succeeded. An in-progress task uses the accent
mark. A failed `todos` call stays the error row, **Update tasks**. A running
one shows the spinner on that same row.

A `delegate` call is one collapsed row. Explore reads **Exploring**, then the
counts it has so far: unique `read_file` and `read_code` paths as files, and `grep` or `find`
calls as searches, as in **Exploring 9 files, 5 searches**. A count of zero is
left off. The line uses `--ink-muted`. General reads **General**, and shows the
running spinner while the call is running. Clicking the row opens the panel:
the description and one row per child tool. General also shows a tool-call
count. A spinner sits on a child step that is still running. A denied or failed step
uses `--danger`. The answer the parent model received is behind an **Answer**
control inside that panel and stays closed until that click. Closing the row
hides the panel. The rows update when `tool_call_updated` refetches the
assistant message. Those refetches collapse to one in flight and one trailing
GET. The review strip refreshes on the first of a burst, then once more when
the burst goes quiet. `turn_finished` refreshes it immediately.

A call that is still `pending` approval and `not_started`, while the session
phase is idle, is the approval bar. It shows the same verb and target, then
**Reject** and **Approve**. A pending `web_search` keeps **Search** and **the
web** on that line, and puts the full query under it so a long query wraps
instead of clipping. A pending `web_fetch` keeps **Fetch** and the host on
that line, and puts the full URL under it the same way. A pending MCP call
keeps **Call** and `server / tool` on that line, puts the arguments under it,
and adds **Allow for this session** beside **Approve**. The labels and the
session list are in [mcp.md](../reach/mcp.md). Each approval bar has `--space-2` under it, the same
space as an edit or shell card, so parallel requests do not touch. A pending `edit_file` uses the same diff card as a
finished edit, built from `old` and `new`: that same path, the counts, the
first 4 lines, and the same 24-line cap. **Reject** and **Approve** sit on that
card. Approve posts `approve`. It uses `--accent` with dark text. Reject posts
`reject`. Either button lightens on hover. The phase
becomes **Thinking** until the resumed turn reports back. If that post fails,
the shell loads the assistant message again. A call that is still `pending`
and `not_started` keeps the bar and returns the phase to idle. A call that is
no longer waiting stays as the fetched row and keeps the thinking phase. A call that ran
without asking stays a result row: `pending` approval with `succeeded`
execution is not a prompt.

When the desktop window is not in front, that pause also posts one OS
notification: **Robi needs approval**, and the verb and target of the first
waiting call. A click focuses the window and selects the session. The bar is
still where the call is approved or rejected. The next `turn_started` clears
that notice so a later pause can post again. A failed turn posts **Robi**
with the failure message the same way, and the in-app notice keeps that
message either way. The browser shell does not post one.

## Mermaid diagrams

A fenced ` ```mermaid ` block renders as a diagram, not as source, everywhere
assistant Markdown is shown: an assistant message in the transcript, a plan
page body, and the markdown file preview in the review file inspector. One
renderer, `AssistantMarkdown`, backs all three, so the fence behaves
identically in each. The file inspector's Diff, Current, and Previous views
still show the raw file lines.

The diagram is drawn client-side, and the `mermaid` package is dynamically
imported, so it loads only the first time a fence renders and never sits in
the main bundle. While the render is in flight the fence shows as an ordinary
code block; when the SVG is ready it replaces the source in a centered,
horizontally scrollable surface (see
[visual-style.md](visual-style.md#mermaid-diagrams)). Mermaid writes
`width="100%"` and no height. WebKit then paints the SVG at its 150px default
and clips the rest, which cuts a sequence diagram off under the participant
boxes. The renderer copies the viewBox width and height onto the root so the
diagram keeps its aspect ratio, and the stylesheet still shrinks it to the
column. A source mermaid cannot
parse keeps the code fence. Mermaid's own error diagram is suppressed, so a
bad diagram does not paint a wide error into the page; the failure is logged.

The theme is the same dark token set as the shell, mapped to mermaid's
`themeVariables`, so diagrams do not fall back to its default palette. The
look is `classic`. Mermaid's default `neo` look adds a light gray drop shadow
that reads as a second fill on the dark shell, worst when a box contains a box.
`securityLevel` stays `strict`: the source is model output, so mermaid keeps
its sanitization.

Both fenced `flowchart` blocks and `sequenceDiagram` blocks use that theme. A
sequence diagram reads its own variable groups — `actor*` for the participant
boxes and lifelines, `signal*` for messages, `note*` for notes, and `labelBox*`
/ `activation*` for the loop/alt frames and the bars inside them — so those are
set explicitly rather than left to mermaid's light defaults. A semicolon ends
a sequence statement, so a semicolon inside a note or message (for example
`unknown;<br/>`) makes the rest of the line fail to parse and the fence stay
on screen as source. A semicolon that begins the next statement is kept. One
that sits in the prose is drawn as a fullwidth semicolon, which is not a
statement break. An actor id that is a sequence keyword is quoted before
render. `Loop` is one of those: the lexer matches `loop` without regard to
case, so `API->>Loop` is read as the start of a loop and the diagram stays
source. The alias after `as` and the message after `:` are unchanged.

Diagram text is the shell's UI sans at 14px, matching the fenced code block it
replaces. The font comes from the top-level `fontFamily` key rather than a
theme variable: sequence diagrams read their own `actorFontFamily` /
`noteFontFamily` / `messageFontFamily`, and mermaid fills those in from the
top-level value.

## Activity and the composer

While the phase is not `idle`, the transcript shows a muted line, **Thinking**
or **Responding**, then **for Ns** counted from the latest message, with
dots that step `.`, `..`, `...` beside it. The count waits until one second
has passed. Reduced motion shows `...` and does not step. That line is the
busy signal.

The ring at the right of that row is the context meter. It is a button.
The arc is the share of the model's context window the next request would
use. The latest assistant `usage.input` is the prompt size through the
request that reported it. Each report is cumulative, so the meter does not
sum them. Characters of that message, every later message, the unstored
echo, and the composer draft, divided by four, estimate what the provider
has not counted yet. The track is `--ink-faint` and the arc is `--ink-muted`.
An empty chat, or a turn that has not reported usage, leaves
the arc empty. A model with no advertised window leaves it empty too.
Clicking the ring opens a popover: used against the window and the percent,
the last turn's input, output, and cached tokens, and the uncounted
estimate when it is not zero. **Compact** in that popover runs the manual
rewrite in [context-management.md](../workspace/context-management.md). It is
disabled while a turn runs or a compact is in flight; while the request is out
the label reads **Compacting…**. Cached is omitted when it is zero. The ring
holds its fill while a turn runs.

A compaction summary is a `user` message the API flags `compaction: true`. The
transcript does not draw it as a user bubble; it paints a **Context compacted**
divider — a hairline rule with the label between it — on that row instead. The
summary text is not shown. The rows it replaced are gone, so a client that
receives `robi.agent.v1.transcript_compacted` refetches the message list.

Unsent text stays with the chat it was typed in. Switching sessions, or moving between the greeting card and the bottom field, restores that chat's draft. A successful send clears it. The new-chat draft is its own slot until the first send creates a row.

The textarea stays editable while a turn runs and while a send is in flight.
Enter does not submit during session load, a send that has not returned, or
while the phase is not `idle`. Until that send returns, the send slot is a
spinner. After it returns, the control becomes **Stop**. A transcript read
already in flight does not clear that phase: the read started before the
local running mark. **Stop** is a filled circle with a rounded square cut out, in the same
slot as the return mark. Stop posts `POST /chat_sessions/{id}/stop` and stays
in that slot until the call returns, which is after the actor has exited. The
client does not send a second instruction while the phase is not `idle`.
Another session can still be running; the lock follows the session on screen.
Selecting it again refetches, and `has_pending_agent` restores the phase when
the actor is still running.

Recents shows the five most recently used sessions. **Show more** reveals the
next ten, then the ten after that, until the list is open. The session on
screen stays in that list when it is older than the window. Switching
workspace returns the list to five.

A session whose agent is still running shows a grayscale spinner on its row
in the sidebar. That is the phase when it is not `idle`, `has_pending_agent`
when the list was loaded with the actor already running, or `turn_display` of
`pending`. Leaving that chat does not drop the spinner. `turn_finished` for
that session still arrives on the open stream and clears the running mark,
without refetching its transcript. `turn_started` sets the mark the same way.
A `robi.session.v1.updated` frame that carries `turn_display` applies that
value to the row, then refetches the session. The actor publishes that frame
after it has gone idle, so the refetch does not report the actor as running.
`awaiting_approval` shows a filled `--accent` dot in the spinner's slot, with
an accessible name that the session is waiting on approval. A fetch that
started before that frame does not put the spinner back. `failed` is returned
and not painted. Message frames for a session that is not on screen
are not applied, so the row does not update on each token. A session refetch
replaces the row when the title, mode, model, grants, running flag, or
`turn_display` change. Reduced motion keeps the ring still.

Mode and the model menu are quiet dropdowns on the left of that row, and
inside the welcome card. The context meter sits on the right of an open
session's row. The welcome card does not show it. Mode is `ask`,
`plan`, or `agent`. The selected mode, and each row in its menu, uses that
mode's color: ask is `--mode-ask`, plan is `--mode-plan`, and agent stays
`--ink-muted`. The model menu's label is the model and effort in effect,
such as `Grok 4.7 Low`. An OpenCode Go model is prefixed `OCG - `, such as
`OCG - Grok 4.7 Low`, so it stays distinct from the same name on another
provider. The same prefix appears in the model menus on General.
Opening it shows a Model row and an Effort row, and
each opens its own list. Model and effort show the value in effect for that
mode: the session override when one is stored, otherwise that mode's
setting, then the fallback setting. **Use default** clears that mode's
session key. A saved session writes the choice with
`PATCH`. A draft keeps it in the client until the first send, which stores
it on `POST /chat_sessions` before the instruction. The dropdowns stay
usable while a turn is running. The actor already built keeps its mode,
model, and tools; the next one reads the new choice. The catalog comes from
`GET /api/v1/models`, and each model includes `context_window`. Settings
hold the fallback model and effort, and an optional model and effort per
mode. The mode rules are in [agent-modes.md](../core/agent-modes.md).

Attached images sit as 32px thumbnails in a row at the top left of the field,
above the text. Text files sit in that same row as `filename (1-10)` chips. The
chip is the same `AttachmentChip` the transcript and the pending echo use.
Hovering a thumbnail shows its remove control; a chip carries its own remove
control. The paperclip accepts PNG, JPEG, WebP, and GIF, plus common source and
text extensions. Pasting those files, or dropping them on the field, adds them
the same way. A screenshot paste with no filename is named `pasted.png`. A
message holds at most eight attachments. A text file larger than 64 KB, one that
contains a NUL byte, or one that would push the attachments past 256 KB in
total, stays in the field and the composer reports why. On send, an image still
travels as a multipart file part; a text file travels in the request's `files`
array as a `FileAttachment`. The paperclip sits in the control row beside the
context meter. A chip can also arrive without a picker: the docs viewer's
line attach hands the composer a ranged `FileAttachment` through a keyed request
queue, which the composer drains into the same list and then focuses the field.
The two paths share `appendAttachments`, so the caps above apply to both. See
[file-attachments.md](file-attachments.md) for the attachment model.

A sent message shows images as a row of 32px thumbnails above the message
text. Each thumbnail loads from
`GET /api/v1/chat_sessions/{id}/images/{image_id}` on the API origin. In the
desktop app that origin is `http://127.0.0.1:<port>`, not the webview, so the
`src` is absolute. A sent message's text attachments show as chips above the
text, from the message's `files` metadata. The message before it is stored shows
the same chips through the pending echo.

In the desktop app the paperclip opens the OS file dialog, so a picked file
carries an absolute path and the server can mark it in-workspace or outside. In a
browser the `<input>` yields only a name, so a picked file is treated as outside.
The chip's tooltip names the workspace path, or says the file came from outside
the workspace. See [file-attachments.md](file-attachments.md).
