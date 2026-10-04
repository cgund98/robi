# Context and compaction

This page defines token accounting for the context meter, when a session
compacts, and what the rewrite keeps. It is **F3.4** in the
[roadmap](../../roadmap.md). The meter drawing lives in
[chat-ui.md](../shell/chat-ui.md). The system prompt, including the plan
checklist, is rebuilt on every request and is specified in
[instructions.md](../core/instructions.md). Shrinking one tool result as it
returns, and keeping that original, is M9
([shell-output.md](../compression/shell-output.md),
[mcp-output.md](../compression/mcp-output.md)).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| How the ring and popover are drawn | [chat-ui.md](../shell/chat-ui.md) |
| Prompt blocks that are not transcript messages | [instructions.md](../core/instructions.md) |
| Provider `usage` on the stream | [providers-streaming.md](../providers/providers-streaming.md) |
| Compressing one tool result before it is appended | M9 compression pages |
| A child agent's own transcript | [subagents.md](../core/subagents.md) |

## Problem

A session that reads files fills the window. The next request then fails, or
the provider drops a prefix the model still needed. gopi waits for `/compact`.
A GUI that only does that lets a long read session die between turns.

The rewrite has to stay a transcript a provider will accept. Cutting in the
middle of a tool round leaves an assistant message whose calls have no
results. Dropping the system prompt does nothing, because that string is not
a message. Keeping every recent tool body and only deleting chat text saves
the least useful bytes.

## Decision

Compaction is a lossy summary of an older prefix. The live transcript is
rewritten. There is no second copy of the dropped messages.

Two triggers call the same rewrite:

- **Auto.** At the start of a user turn, after the user message is stored and
  before the first `generate` of that turn, when the estimate below reaches
  **80%** of the model's `context_window`.
- **Manual.** **Compact** in the context-meter popover. Same cut. The client
  posts `POST /api/v1/chat_sessions/{id}/compact`.

The shell says it happened. The summary is a user message with
`compaction: true`. The transcript paints a **Context compacted** divider on
that message. The route and the auto path both emit
`robi.agent.v1.transcript_compacted` with `{ "session_id", "message_id" }`,
and an open client refetches the message list.

### Estimate

The meter and the trigger share one number.

`used` is the latest assistant `usage.input`, plus the characters of every
message after that assistant message, divided by four. `usage.input` is
already the prompt through that request, so earlier messages are not added
again. When no assistant message has reported usage, `used` is the characters
of the whole transcript divided by four.

Auto-compact runs when `context_window` is known and
`used >= 0.80 * context_window`. A missing window never auto-compacts. Manual
compact does not consult the threshold.

Characters divided by four is the stand-in until the provider counts those
messages. It is the same stand-in [chat-ui.md](../shell/chat-ui.md) uses for
the arc, except the trigger ignores the composer draft. A draft is not in the
next request.

### Cut

Walk backward from the end to user-message boundaries. The tail is every
message from a chosen user message through the end. The prefix is everything
before it.

The chosen boundary is the oldest user message such that the tail's character
estimate is at most **50%** of `context_window`. The current turn is never
in the prefix: the boundary is at or before the last user message, and the
tail always includes that message and everything after it. If that tail alone
is already over half the window, it is still kept whole.

A boundary is illegal when it would separate an assistant message that has
tool calls from the tool messages that answer them. The walk steps back to
the next older user message.

Nothing to compact means the prefix is empty: the transcript is only the
current turn. Manual compact returns `409` with `{"error":"nothing to compact"}`.
Auto compact skips.

### Summary

One model call, no tools, on the session's current model. The prompt is the
prefix rendered as `role: content` lines, then an instruction to write a
summary the next turn can continue from: decisions made, files read or
changed, tool outcomes that still matter, and constraints the user set. The
call's output is the summary message's `content`.

That message is role `user`, so the tail can still start with the user's real
turn. `compaction: true` is how the UI and a later compact tell it from a
person. A later compact may include an older summary in a new prefix; the
flag does not protect it.

The system prompt and the plan checklist are not in the prefix and are not
summarized. The next request sends them again.

### Rewrite

The summary call finishes before any row is deleted. One SQLite transaction
then deletes the prefix rows and inserts the summary at the front of what
remains. A failed summary, a cancel, or a provider error leaves the
transcript as it was.

The cut and the estimate are pure functions in `robi-core`. The summary call,
the transaction, and the route live in `crates/robi`. The loop does not
compact in the middle of a tool round.

### When auto does not run

- The turn is already inside `generate` or tool execution.
- The transcript has a pending approval. Compact waits. A new instruction
  rejects those calls instead, which is a different path.
- `context_window` is absent.
- The prefix is empty.
- This turn already compacted once. If the tail is still over 80%, the turn
  proceeds and the next user message may compact again.

Manual compact is refused with `409` while the actor is running or the
transcript is waiting on approval, same as `POST .../messages`.

## Rejected alternatives

**Manual only, as in gopi.** A read-heavy session hits the window between
turns, and the GUI has no slash command the user is expected to remember.
Manual stays, as the popover action, for a compact the user wants early.

**Structured truncation of old tool results.** Keeps the shape of the
transcript and spends no model call. It also keeps the turns that caused
those calls, and it drops the bodies the model would need to avoid repeating
the work. M9 already truncates and compresses a single result. Compaction
runs because the window is full anyway.

**An archive of the dropped messages.** The UI would show a transcript the
model cannot see, and a later retrieve would undo the compact. The divider
and the summary are the record. M9 originals for compressed tool results are
a different store; compact does not read or delete them. A summary that
needs a dropped body says so in prose, and the model reads the file again.

**Compacting mid tool-loop.** Frees tokens sooner. It also races the append
of tool results and can cut a call from its result. The check stays at turn
start.

**Putting the summary in the system prompt.** Hides it from the transcript
and from the meter. The next edit to the assembler would drop it. A user
message is stored, visible, and counted.

## Interfaces

```rust
/// Where a compact may cut. `prefix` is summarized. `tail` is kept.
pub struct Cut {
    pub prefix: Vec<Message>,
    pub tail: Vec<Message>,
}

/// `None` when the prefix would be empty.
pub fn plan_cut(messages: &[Message], context_window: u64) -> Option<Cut>;

/// Same figure the meter uses, without the composer draft.
pub fn estimate_tokens(messages: &[Message]) -> u64;
```

`POST /api/v1/chat_sessions/{id}/compact` returns `202`
`{"status":"compacting"}` once the actor has accepted the job. Completion is
the `transcript_compacted` event. Failures on the request itself:

| Status | When |
|---|---|
| `400` | `id` is not a UUID |
| `404` | No such session |
| `409` | Actor running, approval pending, or nothing to compact |

A summary that fails after `202` emits `robi.app.v1.error` for that session
and does not rewrite.

The message DTO gains `compaction: true` only on a summary. Other messages
omit the field.

## Failure modes

| What happened | Result |
|---|---|
| Summary call errors or is cancelled | Transcript unchanged. Manual surfaces the error. Auto leaves a trace on the session error event and still runs the user turn |
| Transaction fails after a successful summary | Transcript unchanged. The summary text is discarded |
| Tail is already most of the window | Prefix is summarized if any exists. The turn is not refused |
| No `usage` yet | Character estimate only. Auto may run early or late by that error |
| Model has no `context_window` | No auto. Manual still runs |
| Summary is longer than the prefix | Still stored. The next turn can compact again. No second call in the same turn |
