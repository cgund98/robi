# Subagents

A **subagent** is a child agent Robi spawns to do a bounded piece of work and
report back only an answer. Its value is **context isolation**: the parent gets
the conclusion, not the pile of file bodies and command output that produced it.

You do not call a subagent directly. The model does, through one tool — `delegate`
— which is registered in **agent** mode only.

## Two child modes

`delegate` takes `mode`:

| Mode | What it can do | Model turns | Calls per session |
|---|---|---|---|
| `explore` | Read and search: `read_file`, `read_code`, `list_dir`, `find`, `grep`, `semantic_search`, and the read-only language-server tools | 40 | 6 |
| `general` | The explore set plus a sandboxed `shell` (and `retrieve`) | 50 | 4 |

Both children share a wall-clock cap of **two minutes** per call; the parent turn
cancelling cancels the child. When a call budget is spent, the tool returns
`explore_limit` or `delegate_limit` and the parent continues on its own.

Neither child can edit files, `grant`, `delegate`, or call MCP tools. A child
cannot start another child.

## Fail closed

A child never prompts you. If a child tool would need your approval, it does not
run: it returns `access_denied` and the child continues. The child inherits the
parent session's path rules — including a read grant, read-only — and cannot
widen them. This is deliberate: a subagent that could raise a dialog would let
the model interrupt you from a task you did not start.

## What comes back

The parent model reads a small JSON summary:

```json
{ "mode": "explore", "answer": "...", "tool_calls": 3, "denied": [] }
```

`answer` is the child's final message. Every file body and command output stays
in the child's in-memory transcript and is dropped; the child is not a chat
session and is never persisted.

Each child tool call is also written onto the parent's tool card as a row — tool
name, a one-line target, and a status (`running`, `ok`, `denied`, `failed`) — so
you can watch it work. These rows are not part of the model-visible result.
Several `delegate` calls in one turn may run in parallel.

## Where this is specified

The child policy, the prompt construction, the thoroughness knob, and the
snapshot surfacing are in [Subagents](../design/core/subagents.md). Which tools
each mode registers is in
[Agent modes](../design/core/agent-modes.md).
