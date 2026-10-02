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
| Markdown, syntax highlighting, copy, retry, edit-and-resend | Later on this page |
| Tool-call cards and approval prompts | This page, when tools exist (M3). A tool message with text renders as a muted line |
| Creating the session row | [persistence.md](persistence.md). The shell delays that call |

## Draft session

**New session** does not `POST /chat_sessions`. It selects a client-only draft:
no database row, no sidebar entry, header title “New session”. Choosing it
again while it is already open does nothing.

The first send creates the session, then `POST /chat_sessions/{id}/messages`
with `{ "instruction" }`. On `202` the shell selects that session, marks it
thinking, and keeps a local echo of the user text until a fetched user message
with that text is new in the list. If create succeeds and submit fails, the
session stays and the draft text stays in the composer so the retry is only
the message post.

App load lists sessions and selects the most recent. An empty list opens the
draft.

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

## Activity and the composer

While the phase is not `idle`, the transcript shows a muted line, **Thinking**
or **Responding**, with a small accent pulse. The composer status ring spins
in `--accent`. Idle keeps the quiet ring. Reduced motion leaves the color and
drops the animation.

The textarea and send control are disabled for that whole stretch, and during
session load and other in-flight session requests. Enter does not submit.
There is no stop control. The client does not send a second instruction while
the phase is not `idle`. Another session can still be running; the lock
follows the session on screen. Selecting it again refetches, and
`has_pending_agent` restores the phase when the actor is still running.

Model and effort controls stay as quiet placeholders.
