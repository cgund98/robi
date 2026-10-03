# HTTP API

`robi-api` is a local Axum server. It is the composition root: it wires the
SQLite adapters, the provider clients, and the agent runtime behind an HTTP
surface the desktop shell and the web shell both use. It binds to
`127.0.0.1:1431` by default (override with `ROBI_BIND`; the address must be
loopback).

All routes live under `/api/v1`. The request and response bodies are described by
the OpenAPI document at `openapi/openapi.json`, which is served alongside a
Swagger UI at `/docs`. Every error has one shape: `{"error": "..."}`, with a
status of 400, 404, 409, or 500.

## Health

| Method | Path | Returns |
|---|---|---|
| `GET` | `/api/v1/health` | `200 "Ok"` |

## Workspaces

| Method | Path | Returns |
|---|---|---|
| `POST` | `/api/v1/workspaces` | `201` when the root is new, `200` when already stored. |
| `GET` | `/api/v1/workspaces` | Every workspace. |
| `GET` | `/api/v1/workspaces/{id}` | One workspace. |
| `DELETE` | `/api/v1/workspaces/{id}` | `204`. |
| `GET` | `/api/v1/workspaces/{id}/skills` | The user-invocable skills for the workspace. |
| `GET` | `/api/v1/workspaces/{id}/mcp` | Configured MCP servers and their status. |
| `GET` | `/api/v1/workspaces/{id}/mcp/config` | The user and project MCP config text. |
| `GET` | `/api/v1/workspaces/{id}/index` | The semantic index status, with a cursor. |
| `PUT` | `/api/v1/workspaces/{id}/index` | Pause or resume the index (`state`: `paused` / `running`). |

## Chat sessions

| Method | Path | Returns |
|---|---|---|
| `POST` | `/api/v1/chat_sessions` | `201` and the new session. |
| `GET` | `/api/v1/chat_sessions` | Sessions, optionally filtered by `workspace_id`. |
| `GET` | `/api/v1/chat_sessions/{id}` | One session. Includes `has_pending_agent`. |
| `PATCH` | `/api/v1/chat_sessions/{id}` | Update title, mode, model config, or grants. |
| `DELETE` | `/api/v1/chat_sessions/{id}` | `204`. Cascades to the session's messages. |

## Messages and turns

| Method | Path | Returns |
|---|---|---|
| `GET` | `/api/v1/chat_sessions/{id}/messages` | The transcript, in order. |
| `POST` | `/api/v1/chat_sessions/{id}/messages` | `202` `{"status":"accepted"}`; `409` when the session awaits approval. |
| `GET` | `/api/v1/chat_sessions/{id}/messages/{message_id}` | One message. |
| `POST` | `/api/v1/chat_sessions/{id}/tool_calls/{call_id}` | `202`; settle one paused call. `409` when the actor is running. |
| `POST` | `/api/v1/chat_sessions/{id}/stop` | `202` `{"status":"stopped"}`; cancel the running turn. |
| `GET` | `/api/v1/chat_sessions/{id}/tool_originals/{original_id}` | A stored original (shell or MCP) of a compressed result. |

Sending a message is asynchronous: the route accepts the turn and returns `202`,
then the result arrives on the event stream. A `409` on the message route means
the transcript ends with unresolved tool calls, which is the
[approval invariant](../design/core/agent-loop.md) — settle them first.

## Review

| Method | Path | Returns |
|---|---|---|
| `GET` | `/api/v1/chat_sessions/{id}/review` | The session review: baseline plus hunks. |
| `POST` | `/api/v1/chat_sessions/{id}/review` | `204`; approve or reject a file or one hunk. |

## Settings and models

| Method | Path | Returns |
|---|---|---|
| `GET` | `/api/v1/settings/{key}` | `200`; secret values are omitted. |
| `PUT` | `/api/v1/settings/{key}` | `204`. |
| `DELETE` | `/api/v1/settings/{key}` | `204`. |
| `GET` | `/api/v1/models` | The vendored model catalog (`id`, `display_name`, `context_window`). |

## Event stream

`GET /api/v1/events/stream` is a Server-Sent Events stream with
`Content-Type: text/event-stream`. It carries every state change in the process;
the shell holds one connection and updates from it.

**Query parameters.** Both optional:

- `session_id` — a UUID; limit the stream to one session.
- `event_types` — repeat the parameter to select event types.

**Frame shape.** Each frame is:

```
id: <cloudevents id>
event: <cloudevents type>
data: <full envelope JSON>
```

The envelope is CloudEvents 1.0: `specversion` is `"1.0"`, `id` is a UUIDv7,
`source` is `robi/agent`, `subject` is the session id, `time` is RFC 3339 UTC,
and `data` is the payload. A keep-alive comment is sent every 15 seconds.

**Event types.** A `type` names its `source`:

| Type | Meaning |
|---|---|
| `robi.agent.v1.turn_started` | A turn began. |
| `robi.agent.v1.message_added` | A message was appended. |
| `robi.agent.v1.message_updated` | A message changed. |
| `robi.agent.v1.message_delta` | A streaming text or reasoning delta. |
| `robi.agent.v1.tool_call_updated` | A tool call changed state. |
| `robi.agent.v1.awaiting_approval` | A turn paused for a decision. |
| `robi.agent.v1.turn_finished` | A turn ended, with an outcome. |
| `robi.session.v1.created` | A session was created. |
| `robi.session.v1.updated` | A session's metadata changed. |
| `robi.session.v1.deleted` | A session was deleted. |
| `robi.app.v1.error` | A process-level error. |
| `robi.index.v1.progress` | Semantic index progress for a workspace. |

A `session_id` filter still delivers session lifecycle events and `app.error`,
plus index progress for that session's workspace. A turn outcome in `data` is one
of `complete`, `paused`, `cancelled`, or `failed`.

The bus fans out per subscriber with a bounded queue of 1024; on overflow the
oldest event is dropped, so a slow reader loses deltas rather than stalling the
server. The front end reconnects with exponential backoff.

## Where this is specified

The event envelope, the fan-out, and the shell's subscription are in
[Events and SSE](../design/shell/events-sse.md). The session lifecycle and the
error shape are in [Persistence](../design/persistence/persistence.md). The route
list is generated from the code into `openapi/openapi.json` by
`make openapi-spec`.
