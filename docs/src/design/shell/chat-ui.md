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
| Grant and session-allow editing | `docs/src/design/workspace/permissions.md` (M3) |
| Creating the session row | [persistence.md](../persistence/persistence.md). The shell delays that call |
| `@id` skill mentions and the **Using** row | [skills.md](../reach/skills.md) |
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

Until a prompt is submitted, the main column has no session title. A greeting
sits in the center — **Good morning**, **Good afternoon**, or **Good evening**,
from the local hour — with the composer in a card under it. The first echo or
stored message returns the title, the transcript, and the bottom composer.

The pencil on a session opens a rename dialog with the stored title. The dialog
is a Radix dialog. An unset title starts the field empty. Save sends `PATCH`
with the trimmed title. Cancel and Escape leave the title as it is. An empty
title is refused in the dialog and is not sent.

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
| Select a session, or the event stream opens | `GET /chat_sessions` and `GET /chat_sessions/{id}/messages` |
| `message_added`, `message_updated` | `GET /chat_sessions/{id}/messages/{message_id}`, then upsert that row |
| `turn_finished` | The session and the message list again |
| `robi.session.v1.created`, `robi.session.v1.updated` | That session again. The message list is left as it is |
| `robi.session.v1.deleted` | Drop that session from the list |
| `robi.app.v1.error` | Show `message` on the shell error line |
| Phase is `thinking` or `responding`, and no frame has arrived for 2 seconds | The session and the message list again |

`message_delta` does not change message text. `kind: "reasoning"` sets
**Thinking**. `kind: "text"` sets **Responding**. Other delta kinds are
ignored. `turn_started` sets **Thinking**. `turn_finished` sets `idle`, then
the session refetch restores **Thinking** when `has_pending_agent` is still
true. That flag is still set while the actor emits `turn_finished`, so the
shell reads the session once more and returns to `idle` when the actor has
exited. The 2-second refetch is what paints a stored turn when the frame that
would have loaded it was dropped. A frame resets that wait.

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
it, and the first 4 diff lines. Added and removed lines carry a left accent in
`--diff-add` or `--diff-del`. Clicking the name opens the rest, at most 24
lines, then `N more lines` when the change is longer. A running call shows a spinner.
A failed call shows the verb and target in `--danger`. Clicking a row that has a result or an error opens
the body: numbered file text, match lines, paths, or the error. The row stays
closed until that click.

A finished `write_plan` is a bordered card. The label is **Created Plan**, or
**Updated Plan** when the result status is `updated`. The title is the first
heading in that call's markdown, otherwise `plan_name`, otherwise the file
name. Under it is the first paragraph of the markdown, clamped to three lines.
**View Plan** and **Build** sit at the bottom right, both in `--mode-plan`. **View Plan** replaces
the chat with a page that fills the main column. The header is **Back**, the
plan name, and **Build**. **Back**, or choosing another session, returns to
the chat. The body lists that call's todo steps, then the markdown, rendered
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
assistant message.

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
becomes **Thinking** until the resumed turn reports back. A call that ran
without asking stays a result row: `pending` approval with `succeeded`
execution is not a prompt.

When the desktop window is not in front, that pause also posts one OS
notification: **Robi needs approval**, and the verb and target of the first
waiting call. A click focuses the window and selects the session. The bar is
still where the call is approved or rejected. The next `turn_started` clears
that notice so a later pause can post again. The browser shell does not post
one.

## Activity and the composer

While the phase is not `idle`, the transcript shows a muted line, **Thinking**
or **Responding**, then **for Ns** counted from that turn's user message, with
dots that step `.`, `..`, `...` beside it. The count waits until one second
has passed. Reduced motion shows `...` and does not step. That line is the
busy signal.

The ring at the end of the model row is the context meter. It is a button.
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
estimate when it is not zero. Cached is omitted when it is zero. The ring
holds its fill while a turn runs.

The textarea is disabled for that whole stretch, and during session load and
other in-flight session requests. Enter does not submit. The send control
becomes **Stop**: a square in the same slot as the return mark. Stop posts
`POST /chat_sessions/{id}/stop` and stays in that slot until the call
returns, which is after the actor has exited. The field stays disabled until
then. The client does not send a second instruction while the phase is not
`idle`. Another session can still be running; the lock follows the session
on screen. Selecting it again refetches, and `has_pending_agent` restores
the phase when the actor is still running.

A session whose agent is still running shows a grayscale spinner on its row
in the sidebar. That is the phase when it is not `idle`, or `has_pending_agent`
when the list was loaded with the actor already running. Reduced motion
keeps the ring still.

Mode, model, and effort are quiet dropdowns in that row, and inside the
welcome card. Mode sits on the left. Model, effort, and the context meter sit
on the right. Mode is `ask`, `plan`, or `agent`. The selected mode, and each
row in its menu, uses that mode's color: ask is `--mode-ask`, plan is
`--mode-plan`, and agent stays `--ink-muted`. Model and effort show the
value in effect for that mode: the session override when one is stored,
otherwise that mode's setting, then the fallback setting. **Use default**
clears that mode's session key. A saved session writes the choice with
`PATCH`. A draft keeps it in the client until the first send, which stores
it on `POST /chat_sessions` before the instruction. The dropdowns stay
usable while a turn is running. The actor already built keeps its mode,
model, and tools; the next one reads the new choice. The catalog comes from
`GET /api/v1/models`, and each model includes `context_window`. Settings
hold the fallback model and effort, and an optional model and effort per
mode. The mode rules are in [agent-modes.md](../core/agent-modes.md).
