# How Robi works

Robi is a desktop coding assistant. A Rust core runs an agent loop against a
local workspace, a Tauri shell hosts it, and a React front end renders the
conversation. The design keeps the interesting part — the loop — in a crate that
cannot do I/O, so its behavior is provable in a unit test.

## The layers

```
src-tauri  →  crates/robi  →  robi-core
 (Tauri)      (all I/O)       (the loop)
```

Dependencies point one way and never reverse.

- **`crates/robi-core`** — the agent loop and nothing else: the transcript types,
  the tool trait and registry, approval state, the event sink. It declares four
  traits — `Model`, `Tool`, `MessageStore`, `EventSink` — and implements none of
  them. No network, no filesystem, no Tauri.
- **`crates/robi`** — every implementation that does I/O: providers, the SQLite
  adapters, the HTTP API, the tools, the sandbox, review, LSP, the semantic
  index, skills, MCP, and compression. Each is a module, promoted to its own
  crate only for a real reason.
- **`src-tauri`** — the Tauri application: commands, IPC, wiring.
- **`src`** — the React + TypeScript front end.

Chat HTTP goes through the Vite `/api` proxy to `robi-api`, which is the
composition root. The shell holds one Server-Sent Events connection for updates;
see [HTTP API](../reference/http-api.md).

## The turn

A **turn** runs from a user message to the last assistant message. The loop is a
repeat-until-settled function:

1. Settle any unresolved tool calls. If one still needs a decision, return
   `Paused`.
2. If there is nothing to invoke and nothing was settled, stop.
3. Enforce the model-turn cap.
4. Take one model turn and append it.
5. If the assistant message has no tool calls, the turn is complete.
6. Otherwise process the tool calls and loop, so the model reads the results.

Three properties decide whether the loop is trustworthy, and each is worth
stating:

- **The transcript is the only state.** The loop derives *paused*, *runnable*,
  and *done* from stored messages. Nothing lives in a local that a restart
  loses, so a paused turn survives a restart with no separate bookkeeping.
- **Iterations count model turns, not tool calls.** Tool fan-out inside one turn
  is free; the cap exists to stop a model that calls tools forever.
- **Tool execution is model order, in segments.** A tool declares itself
  concurrent or exclusive; the loop batches consecutive concurrent calls and
  runs an exclusive call alone as a barrier. Results are committed in model
  order, not completion order.

A tool that fails does not abort its siblings or end the run. The failure becomes
a structured result the model can read and correct, and the loop continues.

## The approval invariant

> User input never runs ahead of an unresolved turn.

If the transcript ends with tool calls that are not settled, the next thing that
happens settles them, whatever the user typed. The loop runs until it needs a
decision, returns `Paused`, and waits to be called again. It does not block a
thread on the user and it does not poll. See [Approvals](approvals.md).

## The four traits

The loop sees only interfaces, never a provider or a concrete store. `Model`
supplies one turn (async and cancellable from the start, which is what lets a
provider stream behind the same trait). `Tool` is a name, a description, a JSON
schema, a side-effect-free `requires_approval`, and an `execute`. `MessageStore`
persists the transcript. `EventSink` emits deltas after the store write, so an
event never describes state the store does not have yet.

Because the loop cannot reach the network or the disk, the whole of `robi-core`
is tested against a stub model and an in-memory store:

```sh
cargo test -p robi-core
```

## Where this is specified

The loop algorithm, the trait signatures, the turn state machine, and the tool
execution phases are in
[The agent loop](../design/core/agent-loop.md). The crate layout and the
one-way dependency rule are in the repository's `AGENTS.md`. The streaming
protocol is in
[Providers and streaming](../design/providers/providers-streaming.md).
