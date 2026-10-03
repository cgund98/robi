# Chat UI

This page is how the shell shows a chat with no tools. It covers the draft
session, the transcript the window stores, the activity line, and the composer
lock. Look and tokens stay in [visual-style.md](visual-style.md). Event
delivery stays in [events-sse.md](events-sse.md).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Streaming caret and painting `message_delta` text | Later on this page. The assistant row is stored only when the model stream finishes, so this cut does not paint tokens |
| Scroll-lock that yields when the user scrolls up | Later on this page. The list follows the latest row |
| Syntax highlighting, copy, retry, edit-and-resend | Later on this page. Assistant text is Markdown; highlighting is not |
| Grant and session-allow editing | `docs/design/permissions.md` (M3) |
| Creating the session row | [persistence.md](persistence.md). The shell delays that call |

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

The transcript is HTTP, not the event stream.

| Trigger | Request |
|---|---|
| Select a session, or the event stream opens | `GET /chat_sessions` and `GET /chat_sessions/{id}/messages` |
| `message_added`, `message_updated` | `GET /chat_sessions/{id}/messages/{message_id}`, then upsert that row |
| `turn_finished` | The session and the message list again |
| `session_updated` | That session again. The message list is left as it is |

`message_delta` does not change message text. `kind: "reasoning"` sets
**Thinking**. `kind: "text"` sets **Responding**. Other delta kinds are
ignored. `turn_started` sets **Thinking**. `turn_finished` sets `idle`, then
the session refetch restores **Thinking** when `has_pending_agent` is still
true.

Opening the stream refetches even on the first connect. A frame published
before the socket existed is recovered from the store. Reconnect does not
replay deltas.

## Tool calls

An assistant message renders its text as Markdown, then one row per tool
call. Headings, lists, tables, links, and fenced code are elements. Syntax
highlighting is not applied. A `tool` message is not shown again; the result
lives on the call. User text stays plain. The bottom of a turn is one row: **Worked for Ns** on the left, and a copy
icon on the right when the turn has assistant text. Hovering it says **Copy markdown**.
After a click the tooltip says **Copied**. The button copies that message's
Markdown as stored. A finished turn ends with a muted line, **Worked for
Ns**, measured from the user message to the last message in that turn.
A turn that is still running, or waiting on approval, does not show it.

Tool rows from later iterations of the same turn sit in that same stack, with
no extra gap between quiet rows. A bordered card — an edit or a shell — has a
little space under it, so two panels do not touch. A finished read is a quiet line: an icon, a verb (`Read`, `Grepped`, `Found`,
`Listed`), and the path or pattern. A finished `write_file`, `edit_file`, or
`delete_file` is a bordered card: the path the tool was called with
(`scratch/test.md`, `../gopi/test.md`, `/tmp/test.md`), the `+` / `−` counts beside
it, and the first 4 diff lines. Added and removed lines carry a left accent in
`--diff-add` or `--diff-del`. Clicking the name opens the rest, at most 24
lines, then `N more lines` when the change is longer. A running call shows a spinner.
A failed call shows the verb and target in `--danger`. Clicking a row that has a result or an error opens
the body: numbered file text, match lines, paths, or the error. The row stays
closed until that click.

A `delegate` call is its own card. The header is the description, an **Explore**
or **General** label, the elapsed time while the call is running, and a count.
Explore counts searches once the child has used `grep` or `find`, and tool calls
until then. General counts tool calls. **Explore** uses `--accent`. The body is
one row per child step, with the same verb and target as a parent tool row, and
a spinner on a step that is still running. A denied or failed step uses
`--danger`. When the call succeeds, the rows stay. The answer the parent model
received is behind an **Answer** control and stays closed until that click.
The rows update when `tool_call_updated` refetches the assistant message.

A call that is still `pending` approval and `not_started`, while the session
phase is idle, is the approval bar. It shows the same verb and target, then
**Reject** and **Approve**. A pending `edit_file` uses the same diff card as a
finished edit, built from `old` and `new`: that same path, the counts, the
first 4 lines, and the same 24-line cap. **Reject** and **Approve** sit on that
card. Approve posts `approve`. It uses `--accent` with dark text. Reject posts
`reject`. Either button lightens on hover. The phase
becomes **Thinking** until the resumed turn reports back. A call that ran
without asking stays a result row: `pending` approval with `succeeded`
execution is not a prompt.

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

The textarea and send control are disabled for that whole stretch, and during
session load and other in-flight session requests. Enter does not submit.
There is no stop control. The client does not send a second instruction while
the phase is not `idle`. Another session can still be running; the lock
follows the session on screen. Selecting it again refetches, and
`has_pending_agent` restores the phase when the actor is still running.

A session whose agent is still running shows a grayscale spinner on its row
in the sidebar. That is the phase when it is not `idle`, or `has_pending_agent`
when the list was loaded with the actor already running. Reduced motion
keeps the ring still.

Mode, model, and effort are quiet dropdowns in that cluster, and inside the
welcome card. Mode is `ask`, `plan`, or `agent`. Model and effort show the
value in effect for that mode: the session override when one is stored,
otherwise that mode's setting, then the fallback setting. **Use default**
clears that mode's session key. A saved session writes the choice with
`PATCH`. A draft keeps it in the client until the first send, which stores
it on `POST /chat_sessions` before the instruction. The dropdowns stay
usable while a turn is running. The actor already built keeps its mode,
model, and tools; the next one reads the new choice. The catalog comes from
`GET /api/v1/models`, and each model includes `context_window`. Settings
hold the fallback model and effort, and an optional model and effort per
mode. The mode rules are in [agent-modes.md](agent-modes.md).
