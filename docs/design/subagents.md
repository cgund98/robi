# Subagents

`delegate` hands one bounded task to a child agent and returns only that child's
answer. The parent transcript stays small. The card under the call shows the
child's tool calls as they happen.

This page is the design for **F5.4** in the [roadmap](../roadmap.md). The loop
that runs the child is [agent-loop.md](agent-loop.md). The card is
[chat-ui.md](chat-ui.md). Ask, plan, and agent modes are
[agent-modes.md](agent-modes.md). `delegate` is registered in agent mode.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Ask, plan, and agent mode selection | [agent-modes.md](agent-modes.md) |
| The sandbox a general child's shell uses | [shell-tool.md](shell-tool.md) |
| A separate explore model | Later. The child uses the session's current model |

## Problem

A search that opens twenty files costs twenty tool results in the parent
transcript. The parent model then has to read all of them. A child that can
pause for approval has no one to ask: the user is in the parent turn.

## Decision

One tool, `delegate`, with two modes.

| | `explore` | `general` |
|---|---|---|
| Tools | `read_file`, `list_dir`, `find`, `grep` | those, plus `shell` |
| Model turns | 40 | 50 |
| Wall clock | 2 minutes | 2 minutes |
| Calls per session | 6 | 4 |

`explore` has no shell. Command output is `general`'s job, which is what makes
explore safe to call for a search. Neither mode can edit, call `grant`, or call
`delegate`. A child cannot start another child.

The agent-mode prompt, and the built-in rules when `delegate` is registered,
tell the parent to use `explore` for a search that spans more than a couple of
files. A single known file stays a direct read.

The child is a normal `Agent` with a fresh in-memory transcript and `NopSink`.
It does not see the parent conversation. Its system prompt is the explore
specialist prompt, or a short investigator prompt for `general`. The task string
is its user message. The model is the session's current model, built with that
prompt and the child's tool registry.

The value the parent model reads is:

```json
{ "mode": "explore", "answer": "...", "tool_calls": 7, "denied": [] }
```

`answer` is the child's last assistant message that made no tool calls. File
bodies and command output stay in the child transcript and are dropped when the
child returns.

While the child runs, each of its tool calls is written onto the parent
`ToolCall.subagent` snapshot and the loop emits `ToolCallUpdated`. The shell
already refetches that message. The snapshot is summaries: tool name, a one-line
target, and `running`, `ok`, `denied`, or `failed`. It is not part of the tool
result the model reads. After a refresh the same snapshot is still on the call.

Several `delegate` calls in one turn may run together. Each snapshot hangs off
its own call. Writes to that assistant message share one lock.

### Fail closed

A child tool that would return `NeedsApproval` does not run. It returns
`access_denied` and the child continues. The user is never asked from inside a
child. The child uses the parent session's path rules and cannot widen them.
The parent grants a path first, then delegates.

Hitting the session budget returns `explore_limit` or `delegate_limit` as the
tool result. The parent continues and does the work itself.

A cancelled parent turn cancels the child. The two-minute clock cancels the
child and the tool returns timed out.

## Rejected

- **Two tools, `explore` and `delegate`.** The model has one decision, which
  mode, and the runner is shared. The tool description tells it which mode to
  pick.
- **A child chat session in SQLite.** The parent card only needs the snapshot.
  A second session would put file bodies where a later parent turn could load
  them.
- **A new SSE event for child progress.** `ToolCallUpdated` already means the
  stored call changed. The snapshot is that change.
- **Letting the child ask for approval.** An approval card is a question for
  the user, and a child has no line to them.
- **Giving explore a shell.** OpenCode's explore agent can run bash. The noisy
  part of exploration is file bodies, and a command's output belongs to
  `general`.

## Failure modes

- The child stops at its iteration cap. The parent still receives whatever
  answer was produced, or "stopped before a final answer".
- The child pauses. That is a bug in the fail-closed wrapper. The tool returns
  an error instead of leaving the parent waiting.
- A progress write fails. The in-memory snapshot is still applied when the call
  finishes, so the card is complete after the tool returns.
- The answer is untrusted. A child that read a file can repeat what the file
  said. The parent verifies a claim before it edits.
