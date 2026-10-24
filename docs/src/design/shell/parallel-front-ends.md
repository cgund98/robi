# Parallel front ends

Several Robi processes on one machine: one database, one `~/.robi`. The rule
today is one front end at a time. This page decides how that relaxes — a
**per-session lease**, plus the handle-ownership rule that makes the
per-session `redb` files safe to open from a second process.

The blast radius is small. Nothing in `robi-core`. Nothing moves between blobs
and the chat database. Once ownership is explicit, the alarming part is not
corruption; it is where the refusal lands.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| The turn lifecycle, approvals, tool execution order | [agent-loop.md](../core/agent-loop.md) |
| The per-session actor, `AgentFactory`, interrupt | [chat-runtime.md](chat-runtime.md) |
| Schema, migrations, session lifecycle | [persistence.md](../persistence/persistence.md) |
| The terminal front end, phases T0–T8 | [tui.md](tui.md) |
| Session rows and what survives a restart | [sessions-and-persistence.md](../../concepts/sessions-and-persistence.md) |
| The shared read-modify-write guard | [editing-tools.md](../tools/editing-tools.md) |

## Problem

Two processes is a normal thing to want: the desktop window open, and a terminal
over SSH on the same machine.

Most of it already works. Every process builds its own `AppState` from
`bootstrap::build_app_state`, the session store is SQLite in WAL mode with a
10-second `busy_timeout` (`adapters/sqlite/mod.rs`), and startup is idempotent —
`evacuate_sqlite` returns early once the blob tables are gone. Concurrent reads
are fine and concurrent writers queue.

What is missing is ownership. Every guard that decides who may do what to a
session lives inside one process:

| Guard | Where | Sees another process? |
|---|---|---|
| `Slots` — one actor per session | `adapters/chat_runtime.rs:291` | no |
| `EventBus` — the front end's event stream | `domain/events/bus.rs` | no |
| `path_lock` — one lock per absolute path | `agent/tools/path_lock.rs:13` | no |

So two processes can drive one session and interleave its transcript. That is the
hazard [tui.md](tui.md) names. It is not the first thing that happens.

### The first failure is a read, not a write

Loading a transcript needs an **exclusive open** of that session's blob file:

```text
SqliteMessageStore::messages       adapters/chat_message/store.rs:82
  -> fill                          adapters/chat_message/store.rs:43
    -> SessionBlobs::fill_results         adapters/session_blobs.rs:161
      -> session_db_if_present            adapters/session_blobs.rs:313
        -> session_db                     adapters/session_blobs.rs:293
          -> redb Database::create        adapters/session_blobs.rs:306
```

`Database::create` takes a whole-file exclusive lock, and the second process fails
it with `DatabaseAlreadyOpen` before reading a byte. That becomes
`StoreError::Backend`, then `ServiceError::Unknown` — reported as an internal
error, not as "that session is open elsewhere".

Three details make it worse than it sounds:

- **It hits almost every session.** `fill_results` returns early only when a
  message has no tool calls, so any session that has ever stored a result fails.
- **The handle is never closed.** `dbs` (`session_blobs.rs:34`) is evicted only by
  `remove_session` (`:245`), so the first process holds the file from its first
  render until it exits — not just while that session is on screen.
- **`image-index.redb` is not per session at all** (`:321`, `:332`). It maps an
  image id to a session id, so no per-session rule covers it. The decision below
  removes it rather than deriving a lease for it.

None of this invalidates anything. redb is MVCC with atomic commits, and a failed
`open` leaves the last commit intact. It is a refusal, in the wrong place, with
the wrong message.

## Decision

**A per-session lease file, declared in `domain`, with two scopes; and a rule
that a session's `redb` file may only be open while this process holds it.**

### The lease

`~/.robi/leases/<session_id>.lock`, one OS lock per session through
`std::fs::File::try_lock` (stable since Rust 1.89; the workspace floor is 1.90).
Acquiring truncates the file and writes the holder: pid, scope, database url, and
the time. A refusal reads that back and reports it.

The OS drops a file lock when the descriptor closes, including on `kill -9`.
There is no TTL, no heartbeat, and no stale-lease sweep.

Two scopes, one lock:

| Scope | Held by | Covers |
|---|---|---|
| `Open` | a front end, while a session is on screen | transcript reads and refetches, tool results, originals |
| `Turn` | `SerializedChatRuntime`, from `submit` / `decide` / `compact` | driving a turn |

Both mean the same thing to storage — *this process owns this session* — so one
process refcounts them onto a single OS lock. A background turn does not conflict
with the window that started it.

**`Turn` is exclusion; `Open` is ownership.**

- `Turn` refuses. Submitting, deciding, compacting and deleting are rejected with
  `ServiceError::Conflict`, which `web_api` already maps to 409
  (`web_api/error.rs:15`).
- `Open` never refuses a transcript. It decides whether the blob file may be
  opened, and the answer when it may not is the degraded render below. Calls that
  need the bytes themselves — originals, images — have no degraded form, so they
  are the one place `Open` produces an error.

A **paused turn keeps its `Turn` lease.** The actor goes idle when a turn pauses
for approval, so releasing on actor exit would let a second process resume a call
the first is still showing as pending. Release when the transcript has no
unfinished tool call (`robi_core::message::unresolved_turn`), or when the process
exits, whichever comes first. That ordering is what makes closing a window with a
paused turn transfer ownership cleanly instead of stranding it.

### The invariant

> **A session's `blobs.redb` may be open only while this process holds that
> session's lease.**

That is what turns the lower exclusive lock from an obstacle into a check.
It needs one new operation: `SessionBlobs::close_session`, which drops the cached
`Arc<Database>` without deleting the directory — the eviction at
`session_blobs.rs:245-249`, without the `remove_dir_all`. Release the lease, call
it. Ownership then ends with the lease rather than with the process.

Enforcement sits in the service layer, not in `MessageStore`: the store documents
that its caller holds the lease, because a turn reads through it under `Turn`
while a window reads through it under `Open`.

### Who takes what

| Caller | Scope | On refusal |
|---|---|---|
| TUI / desktop: a session is on screen | `Open`, for as long as it is | falls back, never blocks the render |
| `ChatMessageService::submit_instruction` | `Turn` | `Conflict` |
| `ChatMessageService::decide_tool_call` | `Turn` | `Conflict` |
| `ChatMessageService::compact` | `Turn` | `Conflict` |
| `ChatMessageService::stop` | `Turn` | `Conflict`; the owner stops its own turn |
| `delete_chat_session` | `Turn` | `Conflict` |
| `list_messages` / `get_message` | `Open`, best effort | degraded render |
| originals read (`web_api/chat_message.rs:525`) | `Open` | `Conflict` |
| image read (`web_api/chat_message.rs:446`) | `Open` | `Conflict` |
| `robi-api` | nothing of its own; it is not a window | — |

`delete_chat_session` is not cosmetic. Deleting runs `remove_session_plans` →
`SessionBlobs::remove_session` (`adapters/session_plans.rs:69`,
`session_blobs.rs:245`), which unlinks a directory another process may have open.
The lease makes that ordering explicit instead of accidental.

`robi-api` taking no `Open` is deliberate and is open decision **P1**: the API
serves whatever session it is asked about and has no window, so it cannot know
what a GUI is displaying.

### The degraded transcript

A session another process has open still renders. `MessageStore` keeps the whole
message in `chat_messages.body` except one field: `persist_results` writes results
into blobs, then `take_results` (`session_blobs.rs:515`) strips `call.result`
before the body is serialized (`chat_message/store.rs:157`, `:218`). Content,
tool name, arguments, approval status, execution status, error, truncation and the
subagent snapshot all stay in the row.

So a read that cannot take `Open` renders user and assistant text plus one line
per tool call, its result replaced by `open in another process (pid 1234)`. No
schema change and no second open. The uncontended case is unchanged: taking an
unheld `Open` is immediate, so the normal path is take, read, and hold until the
session leaves the screen.

`SessionLeases::holder(session)` reads the pid without taking the lock, for that
line and for the refusal message.

### The image index

`image-index.redb` is the one `redb` file no session owns, and
`SessionBlobs::get_image` (`:212`) only has an image id. Two processes on two
*different* sessions that both contain images would collide on a file neither
session lease protects.

The pointer moves to SQLite. `chat_image_index(image_id, chat_session_id)` holds
only the mapping, so a lookup is an ordinary WAL query, and the bytes are then
read from that session's `redb` under that session's lease.

```sql
CREATE TABLE chat_image_index (
    image_id        TEXT PRIMARY KEY NOT NULL,
    chat_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE
);
CREATE INDEX idx_chat_image_index_session ON chat_image_index(chat_session_id);
```

The foreign key is the point, not decoration: `foreign_keys` is already on
(`adapters/sqlite/mod.rs`), so deleting a chat session removes its pointers with
the row, and `SessionBlobs::forget_images` (`:260`) plus the `IMAGE_INDEX` table
(`:25`) are deleted rather than ported.

Ownership follows the same split as the rest of the page. Pointers belong to
`BlobImageStore` — the adapter that already holds a pool and a `SessionBlobs` —
not to `SessionBlobs`, which keeps no `SqlitePool` by design. So
`BlobImageStore::image` resolves the id to a session, then calls a
`SessionBlobs::get_image(session, id)` that no longer looks anything up, and
`BlobImageStore::store` writes the row beside the bytes in one step.

**This breaks old images.** See *Compatibility*.

## Compatibility

Robi is used experimentally, so this page takes the break instead of writing a
migration. There is no shim for `image-index.redb`, no backfill of
`chat_image_index`, and no compatibility reader.

What breaks, precisely:

| Data | After this change |
|---|---|
| Images attached to sessions created **before** it | **Lost.** The pointer that resolved `GET /chat_sessions/{id}/images/{image_id}` is gone, so an old bubble's image URL 404s. |
| Tool results and tool originals in those sessions | **Still work.** They are keyed by `{message_id}/{tool_call_id}` and `originals_by_sha` *inside* the per-session file, so nothing about them depends on the global index. |
| Transcripts, titles, settings, workspaces | Unaffected. |

Two ways to clean up, either fine: leave `~/.robi/sessions/image-index.redb` on
disk inert, or delete it on the first start after this lands (decision **P6**).
Do **not** `rm -rf ~/.robi/sessions` to tidy up — that discards tool results for
every existing chat, which this change does not require.

`chat_image_index` is the only schema change on this page. The lease adds no
table — it is a file per session — so nothing else here needs a migration, and
this policy is not a licence to drop unrelated tables.

## Interfaces

```rust
// domain/chat_session/lease.rs — declared here, implemented in `adapters`.
#[async_trait]
pub trait SessionLeases: Send + Sync {
    /// Take or refcount the lease. `Held` names the other process.
    async fn acquire(&self, session: SessionId, scope: LeaseScope)
        -> Result<LeaseGuard, LeaseError>;

    /// The current holder, without taking anything. For the notice and the
    /// refusal message.
    async fn holder(&self, session: SessionId) -> Option<LeaseHolder>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseScope { Open, Turn }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseHolder { pub pid: u32, pub scope: LeaseScope }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    Held(LeaseHolder),
    Io(String),
}

/// Dropping it releases one reference. The last reference closes the `redb`
/// handle and unlocks the file.
pub struct LeaseGuard { /* ... */ }

// adapters/session_blobs.rs — new
impl SessionBlobs {
    /// Drop the cached handle without touching the directory.
    pub fn close_session(&self, session: SessionId) -> Result<(), String>;
}
```

`bootstrap::build_app_state` builds one `Arc<dyn SessionLeases>` and hands it to
`ChatMessageService` (the `Turn` checks and the best-effort `Open`), to
`SerializedChatRuntime` (the paused-turn rule), and to the front ends, which take
`Open` on the session they display.

## Rejected alternatives

1. **A whole-database advisory lock, one process total.** Today's plan of record
   in [tui.md](tui.md). Honest, and 30 lines, but it forbids the case we want and
   *hides* the blob bug: the second process dies at startup instead of failing on
   its first transcript read. A fine stopgap, a wrong design.
2. **Move the blob tables back into the chat database.** This is the change that
   would let reads proceed with no lease at all — WAL gives concurrent readers,
   leaving only "one appender per transcript". Rejected because the adapter
   deliberately keeps per-session bytes out of the shared file, so a write in one
   session does not take the chat database lock, and because it is a rewrite of a
   600-line adapter plus a migration for a property the lease already provides.
3. **A `redb` file per process.** Two copies drift, and merging them is a worse
   problem than a lock.
4. **redb's own multi-process support.** Does not exist, and upstream says there
   are no plans to add it
   ([cberner/redb#932](https://github.com/cberner/redb/issues/932)).
5. **A lease row with a TTL and a heartbeat.** Right answer across *machines*,
   wrong one here: it needs a heartbeat task, a reclamation path and a clock. A
   file lock is released by the OS on crash.
6. **One owning server process, front ends as clients.** The socket
   [tui.md](tui.md) rejects, plus a process lifecycle to manage on top.
7. **A process-local lock** (`path_lock`-style `static`). Invisible to the other
   process, so it prevents nothing that is not already prevented.
8. **A lease table with `BEGIN EXCLUSIVE`.** Works, but a transaction held across
   a whole turn is a long writer on the file every other query needs, and it dies
   differently on restart.

## Failure modes

- **The holder is killed.** The descriptor closes, the OS drops the lock, the next
  process acquires. No cleanup, no zombie lease.
- **You submit to a session another process has open.** `Conflict` naming the pid.
  The draft survives; the send is refused.
- **You read a session another process has open.** It renders from the row, with
  tool results marked `open in another process`. No error.
- **You delete a session another process has open.** Refused, before the unlink.
- **Another process tries to resume your paused turn.** Refused: the paused turn
  keeps `Turn` until it is resolved or the process exits.
- **redb on a filesystem where locking does not work** — a network home, some FUSE
  mounts. redb warns and opens anyway, and that is the one path to real
  corruption. Probe for it at startup and refuse rather than discovering it when
  two processes write.
- **Two processes with different `ROBI_DATABASE_URL`.** Per
  [file-locations.md](../../reference/file-locations.md) the default is
  `sqlite://robi.db` *relative to the working directory*, so two terminals started
  in different directories are two different session sets. Lease files are shared
  but never contended. This is not "the same app twice"; say so instead of
  implying it is safe.
- **A background turn in a process whose window shows another session.** The
  `Turn` and `Open` scopes refcount onto one OS lock, so it is the same owner.
- **`EventBus` is per process.** A rename in one window appears in the other only
  when that window re-lists. Stale, not wrong.
- **`path_lock` is per process.** Two sessions in two processes can interleave a
  read-modify-write on one file and lose a write, silently. Out of scope here; the
  fix is the same `try_lock` pattern at `agent/tools/path_lock.rs:13`.
- **`LspHub`, `McpHub`, `IndexHub` are per process.** Two of each: two index
  watchers on one workspace, two sets of MCP children. The index connection sets
  WAL without a `busy_timeout` (`robi-index/src/store.rs:50`), so concurrent
  indexing surfaces `SQLITE_BUSY` instead of waiting. One line, needed before this
  ships.
- **`config.toml` and `secrets.toml` are last-write-wins** through an atomic
  rename: no corruption, but two settings screens overwrite each other.
- **An old chat's image 404s.** Deliberate, once, per *Compatibility*: the
  pointer that resolved it was in the file this change deletes. Tool results in
  the same chat still render.
- **A restart reconciles turn display.** `ChatSessionService::reconcile_turn_displays`
  can flip a turn another process is running to `idle`. Display only; the owning
  actor rewrites it.

## Build order

1. `domain` lease trait, `adapters` file implementation, in-process refcount, and
   the tests. No caller changes and no behaviour change.
2. `SessionBlobs::close_session`, and the invariant applied where a session is
   displayed: the terminal shell's open session first, then the desktop window.
3. `Turn` in `SerializedChatRuntime` including the paused-turn rule, and
   `delete_chat_session` ordered behind the lease.
4. `chat_image_index` and `BlobImageStore` owning the pointer; delete the
   `IMAGE_INDEX` table and `forget_images`. No migration — see *Compatibility*.
5. The degraded transcript and the `open in pid N` line.
6. The index `busy_timeout`, and `path_lock` if concurrent edits matter.

## Documentation

This page is the decision; the pages that describe today's code are updated when
the step that changes the code lands, per the *write what the code does* rule.
Pending:

| Page | Step | Change |
|---|---|---|
| [persistence.md](../persistence/persistence.md) | 4 | The `image-index.redb` paragraph becomes the `chat_image_index` table and the foreign key. |
| [file-locations.md](../../reference/file-locations.md) | 4 | Drop the `~/.robi/sessions/image-index.redb` row. |
| [tui.md](tui.md) | 2 | Its *one front end at a time* rule becomes the lease. |
| [file-locations.md](../../reference/file-locations.md) | 1 | Add `~/.robi/leases/<session_id>.lock`. |

## Open decisions

| # | Decision | Notes |
|---|---|---|
| P1 | Does `robi-api` take `Open`? | No: it serves whatever session it is asked about. The GUI therefore gets turn-level protection only until a client id or a "viewing" call exists. |
| P2 | Where the lock file lives | `~/.robi/leases/<id>.lock`, not inside `sessions/<id>/`, so a session that stores nothing grows no blob directory and a delete never unlinks a held file. |
| P3 | Does the refusal name the scope? | The file records it; the user-facing sentence is the same either way, because a user does not distinguish `Open` from `Turn`. |
| P4 | May a second process steal an idle lease? | No. Stealing needs liveness detection, which holding the file already gives. |
| P5 | Two views of one session inside **one** process? | Nothing to do: one owner, so tabs and splits need no lease at all. |
| P6 | Delete the stale `image-index.redb` on start, or leave it? | Leave it inert is zero risk and zero code; deleting it is one line and stops it being mistaken for live state. It holds no bytes either way. |

## Relationship to the terminal shell

[tui.md](tui.md) assumed a whole-database lock and called a second front end "a
documented hazard". Under this page the terminal takes `Open` on the session it
displays and `Turn` through the runtime, and the hazard becomes a `Conflict` with
a pid in it. The lease module is therefore needed earlier than the rest of this
page: a terminal that cannot read its own transcript is not a skeleton worth
shipping.
