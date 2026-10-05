# HTTP API

The local API is an Axum server. `bootstrap` wires the SQLite adapters, the
provider clients, and the agent runtime. The desktop app serves that router in
the Tauri process. `robi-api` serves the same router for a browser UI and for
headless use.

The desktop app binds `127.0.0.1:1431`, then the next ports through `1450` when
`1431` is taken, and tells the webview the bound origin. `robi-api` does the
same unless `ROBI_BIND` is set, in which case that loopback address is used
exactly and startup fails if it is taken. The browser UI reaches the API through
the Vite proxy on `1431`, so leave `1431` free when you use `pnpm dev` or
`ROBI_EXTERNAL_API=1`.

Responses include CORS for `http://localhost:1430`, `http://127.0.0.1:1430`,
`http://tauri.localhost`, `https://tauri.localhost`, and `tauri://localhost`
(the macOS and Linux packaged window). Other origins are refused.

All routes live under `/api/v1`. The request and response bodies are described by
the OpenAPI document at `openapi/openapi.json`, which is served alongside a
Swagger UI at `/docs`. Every error has one shape: `{"error": "..."}`, with a
status of 400, 404, 409, or 500. A response outside 2xx is logged at warning. A
handler that takes 2 seconds or longer is logged at warning with its duration.

## Health

| Method | Path | Returns |
|---|---|---|
| `GET` | `/api/v1/health` | `200 "Ok"`. The shell polls this every 5 seconds and treats a non-200 or a wait past 3 seconds as unreachable. |

## Workspaces

| Method | Path | Returns |
|---|---|---|
| `POST` | `/api/v1/workspaces` | `201` when the root is new, `200` when already stored. |
| `GET` | `/api/v1/workspaces` | Every workspace. |
| `GET` | `/api/v1/workspaces/{id}` | One workspace. |
| `DELETE` | `/api/v1/workspaces/{id}` | `204`. |
| `GET` | `/api/v1/workspaces/{id}/skills` | The user-invocable skills for the workspace. |
| `GET` | `/api/v1/workspaces/{id}/mcp` | Configured MCP servers and their status. |
| `POST` | `/api/v1/workspaces/{id}/mcp` | Start this workspace's MCP servers and close every other workspace's. |
| `GET` | `/api/v1/workspaces/{id}/mcp/config` | The user and project MCP config text. |
| `GET` | `/api/v1/workspaces/{id}/index` | The semantic index status, with a cursor. |
| `PUT` | `/api/v1/workspaces/{id}/index` | Pause or resume the index (`state`: `paused` / `running`). |
| `GET` | `/api/v1/workspaces/{id}/docs` | The markdown files in the workspace, gitignore respected. |
| `GET` | `/api/v1/workspaces/{id}/docs/{path}` | One markdown file's text. |
| `GET` | `/api/v1/workspaces/{id}/docs/search` | Markdown search. `engine` is `semantic` (default, the index) or `ripgrep` (a literal scan that does not start the index). |

## Chat sessions

| Method | Path | Returns |
|---|---|---|
| `POST` | `/api/v1/chat_sessions` | `201` and the new session. |
| `GET` | `/api/v1/chat_sessions` | Sessions, optionally filtered by `workspace_id`. |
| `GET` | `/api/v1/chat_sessions/{id}` | One session. Includes `has_pending_agent` and `turn_display`. |
| `PATCH` | `/api/v1/chat_sessions/{id}` | Update title, mode, model config, or grants. |
| `DELETE` | `/api/v1/chat_sessions/{id}` | `204`. Cascades to the session's messages. |

## Messages and turns

| Method | Path | Returns |
|---|---|---|
| `GET` | `/api/v1/chat_sessions/{id}/messages` | The transcript, in order. |
| `POST` | `/api/v1/chat_sessions/{id}/messages` | `202` `{"status":"accepted"}`. A paused turn rejects the pending calls, then runs this instruction. |
| `GET` | `/api/v1/chat_sessions/{id}/messages/{message_id}` | One message. |
| `POST` | `/api/v1/chat_sessions/{id}/tool_calls/{call_id}` | `202`; settle one paused call. `409` when the actor is running. |
| `POST` | `/api/v1/chat_sessions/{id}/stop` | `202` `{"status":"stopped"}`; cancel the running turn. |
| `POST` | `/api/v1/chat_sessions/{id}/compact` | `202` `{"status":"compacting"}`; summarize the older prefix. `409` when the actor is running, the turn is awaiting approval, or nothing is compactable. |
| `GET` | `/api/v1/chat_sessions/{id}/tool_originals/{original_id}` | A stored original (shell or MCP) of a compressed result. |

Sending a message is asynchronous: the route accepts the turn and returns `202`,
then the result arrives on the event stream. If the transcript is paused on
approval, that same submit rejects the waiting calls and then runs the new
instruction. See the [approval invariant](../design/core/agent-loop.md).

## Review

| Method | Path | Returns |
|---|---|---|
| `GET` | `/api/v1/chat_sessions/{id}/review` | Changed paths for the session: path, status, and line counts. |
| `GET` | `/api/v1/chat_sessions/{id}/review/file?path=` | One file's baseline, current text, lines, and hunks. `404` when that path has no remaining changes. |
| `POST` | `/api/v1/chat_sessions/{id}/review` | `204`; approve or reject a file or one hunk. |

## Settings and models

| Method | Path | Returns |
|---|---|---|
| `GET` | `/api/v1/settings?key=` | `200` array, one entry per repeated `key`, in request order. Secret values are omitted. |
| `GET` | `/api/v1/settings/{key}` | `200`; secret values are omitted. |
| `PUT` | `/api/v1/settings/{key}` | `204`. |
| `DELETE` | `/api/v1/settings/{key}` | `204`. |
| `GET` | `/api/v1/models` | The vendored model catalog (`id`, `display_name`, `context_window`). |

## Event stream

`GET /api/v1/events/stream` is a Server-Sent Events stream with
`Content-Type: text/event-stream`. It carries every state change in the process;
the shell holds one connection and updates from it.

**Query parameters.** Both optional:

- `session_id` — a UUID; limit the stream to one session. The shell does not send this.
- `workspace_id` — a UUID. Every session frame is delivered. Index and MCP frames are limited to this workspace. The shell sends the open workspace.
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
| `robi.index.v1.progress` | Semantic index progress for a workspace. The shell refetches `GET /workspaces/{id}/index`, reusing a successful response for 10 seconds. The stream publishes once when it opens, then again as the index changes. |

A `session_id` filter still delivers session lifecycle events and `app.error`,
plus index progress for that session's workspace. The shell subscribes with
`workspace_id` and leaves transcript fetches to the session on screen. A turn outcome in `data` is one
of `complete`, `paused`, `cancelled`, or `failed`.

The bus fans out per subscriber with a bounded queue of 1024. On overflow a
queued `message_delta` is dropped first, and otherwise the oldest event, so a
slow reader loses deltas rather than stalling the server. The front end
reconnects with exponential backoff.

## Where this is specified

The event envelope, the fan-out, and the shell's subscription are in
[Events and SSE](../design/shell/events-sse.md). The session lifecycle and the
error shape are in [Persistence](../design/persistence/persistence.md). The route
list is generated from the code into `openapi/openapi.json` by
`make openapi-spec`.
