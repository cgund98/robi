# Subagents

`delegate` hands one bounded task to a child agent and returns only that child's
answer. The parent transcript stays small. The card under the call stays
collapsed to **Explore** or **General**, with the running spinner while the
child works. Opening it shows the child's tool calls.

This page is the design for **F5.4** in the [roadmap](../../roadmap.md). The loop
that runs the child is [agent-loop.md](agent-loop.md). The card is
[chat-ui.md](../shell/chat-ui.md). Ask, plan, and agent modes are
[agent-modes.md](agent-modes.md). `delegate` is registered in agent mode.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Ask, plan, and agent mode selection | [agent-modes.md](agent-modes.md) |
| The sandbox a general child's shell uses | [shell-tool.md](../tools/shell-tool.md) |
| A separate explore model | Later. The child uses the session's current model |
| MCP tools on the child | [mcp.md](../reach/mcp.md). Neither child mode receives them |
| Skill files, `/id`, and the `skill` tool | [skills.md](../reach/skills.md) |

## Problem

A search that opens twenty files costs twenty tool results in the parent
transcript. The parent model then has to read all of them. A child that can
pause for approval has no one to ask: the user is in the parent turn.

## Decision

One tool, `delegate`, with two modes.

| | `explore` | `general` |
|---|---|---|
| Tools | `shell` (`ShellAccess::ReadOnly`) and `retrieve` | the same |
| Model turns | `subagent_max_iterations`, default 50 | the same setting |
| Wall clock | `subagent_timeout_seconds`, default 120 | the same setting |
| Calls per session | 6 | 4 |

**Both modes hold a read-only shell.** A child cannot ask for approval, and a
sandboxed write inside the workspace needs no approval, so a writable child
could edit files and leave the workspace changed after its summary came back.
Read-only in the profile is what stops that, not the wrapper. A child that
finds a change to make names it in the summary; the parent applies it with its
own approval path. That is the price of having no approval channel, and it is
better than a silent edit.

The child schema has no `unsandboxed`, no `write_paths`, and no unrestricted
network option at all — removing the key is stronger than rejecting the value.
Build and test commands fail in a child for the same reason they fail in Ask and
Plan: they write `target/`.

Neither mode can edit, call `grant`, call `delegate`, call `skill`, call
`web_search` or `web_fetch`, or call an MCP tool. A child cannot start another
child. The child prompt does not include the skill catalog and is written for
the shell: what to run to search, how to read a window, and that the parent
makes the change.

With `--features read-tools`, `semantic-index`, or `lsp-tools`, the
corresponding tools join both child registries too. See
[feature-switches.md](feature-switches.md).

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

Read-only narrows writes; fail-closed still bounds reads. A child that asks for
a path outside the session's rules needs approval for `read_paths`, so
`FailClosed` turns that into `access_denied` rather than a prompt. Both
invariants have a test: one asserts the read-only profile for each child mode,
and one asserts no child tool schema carries an escalation key.

Hitting the session budget returns `explore_limit` or `delegate_limit` as the
tool result. The parent continues and does the work itself.

A cancelled parent turn cancels the child. `subagent_timeout_seconds` (default
two minutes) cancels the child and the tool returns timed out.

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
- **Witholding the shell from explore once the read tools went.** Removing
  `read_file`, `grep`, `find`, `list_dir`, and `read_code` would leave explore
  with no tools at all, so both modes get the read-only shell. The distinction
  between the modes is now the prompt and the call budget, not the tool set.
- **A writable `general` child.** A sandboxed write needs no approval, so the
  child would edit with nothing to audit it: its transcript is in memory and
  the parent sees only the summary.

## Failure modes

- The child stops at its iteration cap. The parent still receives whatever
  answer was produced, or "stopped before a final answer". That cap is logged
  at warn. A finished child is logged at info with its mode and tool-call
  count. A timeout is warn. A pause or any other failure is error.
  Cancellation is info.
- The child pauses. That is a bug in the fail-closed wrapper. The tool returns
  an error instead of leaving the parent waiting.
- A progress write fails. The in-memory snapshot is still applied when the call
  finishes, so the card is complete after the tool returns.
- The answer is untrusted. A child that read a file can repeat what the file
  said. The parent verifies a claim before it edits.
