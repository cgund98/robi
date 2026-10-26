# Parallel front ends

Several Robi processes on one machine: one database, one `~/.robi`. Two front
ends now run at once. Ownership is a **lease per resource**, plus the
handle-ownership rule that makes a session's `redb` file safe to open from a
second process.

The blast radius was small. One data variant in `robi-core` (`ToolError::Locked`),
nothing moved between blobs and the chat database, and the alarming part turned
out not to be corruption but where the refusal lands: it used to arrive as
`Internal server error` at startup.

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

One OS lock file per resource under `~/.robi/locks/`, through
`std::fs::File::try_lock` (stable since Rust 1.89; the workspace floor is 1.90).
Acquiring truncates the file and writes the holder: pid, scope, database url, and
the time. A refusal reads that back and reports it.

| Resource | File | Held by | Wait |
|---|---|---|---|
| Session `S` | `locks/sessions/<S>.lock` | every process with `S`'s `blobs.redb` open | none |
| Workspace file `P` | `locks/paths/<sha256(P)>.lock` | an edit's read-modify-write | 10 s |
| Startup work | `locks/startup.lock` | migrations, evacuation, prune, repair | 10 s |
| Settings write | `locks/settings.lock` | a `config.toml` or `secrets.toml` write | 10 s |

Two kinds of wait, on purpose. **Ownership** (a session) refuses at once: the
holder is mid-turn, so waiting only delays a decision already made. **Edits** wait
`LOCK_TIMEOUT`, ten seconds, the same order as the pool's `busy_timeout`: losing a
race to another process should not fail a turn over a write that takes
milliseconds. A tool that waits out the window returns `ToolError::Locked`, and
the refusal is a sentence naming the path and the pid.

**An acquire timeout is not a lease TTL.** The wait bounds how long a caller
retries; nothing expires a held lock. The OS drops a file lock when the descriptor
closes, including on `kill -9`, so there is no heartbeat and no stale-lease sweep.

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
- `Open` never refuses a transcript. `ChatMessageService::list_messages` takes it
  best effort: when another process has the session, the read goes through
  `MessageStore::messages_without_results`, which leaves `call.result` empty, and
  the card renders `open in another process (pid N)`. Calls that need the bytes
  themselves — originals, images — have no degraded form, so they are the one
  place `Open` produces an error.

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
| TUI: a session is on screen | `Open`, for as long as it is | the degraded render, plus one footer notice |
| `ChatRuntime::submit` / `decide` / `compact` | `Turn`, held in the slot | `Conflict` with the pid |
| `ChatMessageService::stop` | `Turn` already held by the runtime | `Conflict`; the owner stops its own turn |
| `delete_chat_session` | `Turn` | `Conflict`, before the unlink |
| `prune_excess_sessions` | through `delete_chat_session` | the session is kept, startup continues |
| `reconcile_turn_displays` | `Turn` per session, no wait | the session is skipped |
| `ChatMessageService::list_messages` | `Open`, best effort | `messages_without_results` |
| the edit tools, the docs editor, the review reader | a path lock, 10 s | `ToolError::Locked`, and the card says so |
| `robi-api` | nothing of its own; it is not a window | — |

`prune_excess_sessions` matches the `Conflict` rather than propagating it: one
session in use must not abort startup for the rest, and the workspace is only one
over its limit until a later start.

`reconcile_turn_displays` is the rule this page learned the hard way. A session
another process is driving must not be repaired underneath it, or startup writes
`idle` over a running turn. So the `Turn` is taken *before* the read and a refusal
skips the session. The owner writes its own display, and the next start repairs
the row if the owner exited first.

The walk is scoped by `turn_settled`, which the same write as `turn_display`
stores: only a `pending` display, written while a turn runs, leaves a row for the
next start to read. It used to read every session, and that read reached
`fill_results` and opened each session's blob file; the second process then exited
1 on the first session it did not own, reporting a corrupt database. The read is
now `messages_without_results`, since every field the repair looks at is in the
message body, so no blob file is opened and no handle is left behind.

`delete_chat_session` is not cosmetic. Deleting runs `remove_session_plans` →
`SessionBlobs::remove_session` (`adapters/session_plans.rs:69`,
`session_blobs.rs:245`), which unlinks a directory another process may have open.
The lease makes that ordering explicit instead of accidental.

`robi-api` takes no `Open` of its own, which is open decision **P1**: the API
serves whatever session is asked for and has no window, so it cannot know what a
GUI is displaying. It inherits the transient `Open` inside `list_messages`, so a
shared database still gets the degraded transcript rather than a raw `redb`
refusal.

**Known gap: originals and images.** Those reads go straight to `SessionBlobs`
and take no lease, so a second process asking for one gets `redb`'s own
`DatabaseAlreadyOpen` rather than a sentence naming the pid. Both are only reached
from an expanded card in a session the other process has, so the honest fix is to
give `BlobImageStore` and `BlobOriginals` the `Locks` handle and take `Open` the
way `list_messages` does. Not done here.

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
unheld `Open` is immediate.

That read goes through `MessageStore::messages_without_results`, a defaulted trait
method: `SqliteMessageStore` overrides it to skip `fill`, and every other store is
right to inherit `messages` because it keeps results inline.

`Locks::holder(resource)` reads the pid without taking the lock, for that line and
for the refusal message.

### Where a refusal surfaces

Two surfaces in the terminal, and they mean different things.
`Status::set_error` is for a fact about the process — a failed probe, an
`APP_ERROR` frame. `Notice` is for the last thing the reader asked for, and it
sits in the footer next to the composer.

| Refusal | Surfaces as |
|---|---|
| `submit` / `decide` / delete hits a held `Turn` | footer notice *and* the strip: both `that chat is open in another process (pid 4812)`, with the draft left in the composer |
| a tool times out on a path lock | the tool card, and the strip once per call, both `src/lib.rs is held by another process (turn 4812)` |
| a session is `Open` elsewhere | one footer notice on selection, and the card line above |

`ServiceError::user_message` drops the `Conflict: ` prefix on the way to the
footer, so the sentence reads as prose rather than as a display impl.

The path-lock refusal is typed rather than matched by text. `ToolError::Locked`
is a variant whose `kind()` is `"locked"`, so the TUI reads the same field the
model does and the payload the model receives is one it can branch on: wait and
retry, or choose another file. No app-level error event is published for it: the
tool result is already on screen in both front ends, and a global banner for one
contended file would be louder than the event warrants.

### The image index

`image-index.redb` was the one `redb` file no session owned: it mapped an image id
to a session id, so two processes on two *different* sessions that both contained
images could collide on a file no session lease covered. It is also why the file
had to be held open for the whole process life, which is the exclusion this page
removes.

It is deleted, and the pointer moves nowhere: `images` and `image_types` are
already keyed by image id *inside* `sessions/<id>/blobs.redb`, so the session file
is its own index. The map existed only because the read had no session id, and
every caller has one — `Message`'s `images` list, or the route's path.

So the ports take the session:

```rust
async fn image(&self, session: SessionId, id: &str)
    -> Result<Option<(String, Vec<u8>)>, ProviderError>;

async fn store(&self, session: SessionId, id: &str, media_type: &str, bytes: Vec<u8>)
    -> Result<(), ProviderError>;
```

Both are in `agent/providers/images.rs`. `SessionBlobs::get_image` takes the
session and stops looking anything up; `put_image` loses its second write;
`IMAGE_INDEX`, `Inner::image_index`, `image_index()` and `forget_images` are
deleted. `SessionBlobs::remove_session` already unlinks the directory, so cleanup
needs nothing new.

A pleasant consequence: an image id belonging to another chat now reads as
missing rather than resolving. The route used to discard the session with
`let _ = session;` and a comment saying no check was needed.

**This breaks old images.** See *Compatibility*.

## Compatibility

Robi is used experimentally, so this page takes the break instead of writing a
migration. There is no shim for `image-index.redb`, no import, and no
compatibility reader.

What breaks, precisely:

| Data | After this change |
|---|---|
| Images attached to sessions created **before** it | **Lost.** The pointer that resolved `GET /chat_sessions/{id}/images/{image_id}` is gone, so an old bubble's image URL 404s. |
| Tool results and tool originals in those sessions | **Still work.** They are keyed by `{message_id}/{tool_call_id}` and `originals_by_sha` *inside* the per-session file, so nothing about them depends on the global index. |
| Transcripts, titles, settings, workspaces | Unaffected. |

`~/.robi/sessions/image-index.redb` is left inert on disk (decision **P6**).
Nothing reads or writes it, and deleting it by hand is harmless. Do **not**
`rm -rf ~/.robi/sessions` to tidy up — that discards tool results for every
existing chat, which this change does not require.

No schema change: the lease is a file per session, and the image pointer is the
session file itself.

## Interfaces

```rust
// domain/chat_session/lease.rs — declared here, implemented in `adapters/locks.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Resource {
    Session(SessionId),
    Path(PathBuf),
    Startup,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope { Open, Turn }

/// Whether to wait for a held lock. `None` for ownership, `UpTo` for an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait { None, UpTo(Duration) }

pub struct LeaseHolder { pub pid: u32, pub scope: Scope }
pub enum LeaseError { Held { resource: String, holder: LeaseHolder }, Io(String) }

pub trait Locks: Send + Sync {
    fn try_acquire(&self, resource: &Resource, scope: Scope) -> Result<LeaseGuard, LeaseError>;
    fn holder(&self, resource: &Resource) -> Option<LeaseHolder>;
}

/// Take the lock, sleeping on the runtime rather than the thread.
pub async fn acquire_waiting(
    locks: &dyn Locks,
    resource: &Resource,
    scope: Scope,
    wait: Wait,
) -> Result<LeaseGuard, LeaseError>;

/// `~/.robi/locks/<relative>`; `path_key` hashes the absolute path.
pub fn lock_path(settings_dir: &Path, resource: &Resource) -> PathBuf;

// adapters/session_blobs.rs
impl SessionBlobs {
    /// Drop the cached handle without touching the directory.
    pub fn close_session(&self, session: SessionId) -> Result<(), String>;
}
```

`Locks` is sync because `SessionBlobs` is sync and calls it from
`spawn_blocking`; the waiting loop is async and sits beside it. `LeaseError::conflict`
is the one place a refusal becomes `ServiceError::Conflict`, so the TUI footer and
the 409 body are the same sentence. `MemoryLocks` grants everything and is what a
test passes; `FileLocks` is the real one.

`adapters::locks::FileLocks` holds `Mutex<HashMap<Resource, Entry>>` for the
in-process refcount, one `Arc<File>` per held resource, and a release hook the
composition root supplies, so the last drop calls `SessionBlobs::close_session`
before the kernel unlocks the file.

`bootstrap::build_app_state` builds one `Arc<dyn Locks>` and hands it to
`ChatSessionService` (delete, prune, reconcile), to `ChatMessageService` (the
best-effort `Open`), to `SerializedChatRuntime` (the `Turn` and the paused-turn
rule), to `web_api::AppState`, to the TUI, and to `path_lock::init` for the edit
tools.

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
- **You read a session another process has open.** It renders from the chat
  database, with each tool result replaced by `open in another process (pid N)`.
  No error, and one footer notice when the session is selected. Opening such a
  card's original output, or an image in it, reports `redb`'s refusal instead: see
  *Known gap*.
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
- **`McpHub` is per process.** Two processes mean two sets of MCP children. The
  hubs that used to sit beside it, `LspHub` and `IndexHub`, are deleted with M7.
- **`config.toml` and `secrets.toml` are last-write-wins** through an atomic
  rename: no corruption, but two settings screens overwrite each other.
- **An old chat's image 404s.** Deliberate, once, per *Compatibility*: the
  pointer that resolved it was in the file this change deletes. Tool results in
  the same chat still render.
- **A restart reconciles turn display.** `ChatSessionService::reconcile_turn_displays`
  repairs a session left `pending`, and skips one another process holds. Display
  only; the owning actor rewrites it.

## Build order

All six steps are in. Each landed with its tests and its page.

1. `domain/chat_session/lease.rs` + `adapters/locks.rs`: the resources, the
   refcount, the holder line, the path hash, the startup prune, the filesystem
   probe, and the bounded wait.
2. `SessionBlobs::close_session` and the release hook in bootstrap, so the
   handle's lifetime is the lock's.
3. `Turn` in `SerializedChatRuntime` including the paused-turn rule;
   `delete_chat_session` and prune behind it; `reconcile_turn_displays` taking
   `Turn` per session and skipping what it cannot take. This is the step that
   stopped a second process failing at boot.
4. The global image index deleted and `BlobImageStore` reading through the
   session file.
5. The degraded transcript and the `open in another process (pid N)` line.
6. `path_lock` across processes, with `ToolError::Locked`.

## Documentation

| Page | Change |
|---|---|
| [persistence.md](../persistence/persistence.md) | The session file is the image index; no shared `image-index.redb`. |
| [file-locations.md](../../reference/file-locations.md) | The `locks/` tree replaces `image-index.redb`. |
| [tui.md](tui.md) | The one-front-end rule is gone; a refusal surfaces as a notice and a card line. |
| [terminal.md](../../guides/terminal.md) | *Two terminals*, in place of *One at a time*. |
| [AGENTS.md](../../../../AGENTS.md) | The command table no longer says one at a time. |

## Open decisions

| # | Decision | Notes |
|---|---|---|
| P1 | Does `robi-api` take `Open`? | No: it serves whatever session it is asked about. The GUI therefore gets turn-level protection only until a client id or a "viewing" call exists. |
| P2 | Where the lock file lives | `~/.robi/locks/sessions/<id>.lock`, not inside `sessions/<id>/`, so a session that stores nothing grows no blob directory and a delete never unlinks a held file. |
| P3 | Does the refusal name the scope? | The file records it; the user-facing sentence is the same either way, because a user does not distinguish `Open` from `Turn`. |
| P4 | May a second process steal an idle lease? | No. Stealing needs liveness detection, which holding the file already gives. |
| P5 | Two views of one session inside **one** process? | Nothing to do: one owner, so tabs and splits need no lease at all. |
| P6 | Delete the stale `image-index.redb` on start, or leave it? | Leave it inert is zero risk and zero code; deleting it is one line and stops it being mistaken for live state. It holds no bytes either way. |

## Relationship to the terminal shell

[tui.md](tui.md) assumed a whole-database lock and called a second front end "a
documented hazard". The terminal now takes `Open` on the session it displays and
`Turn` through the runtime, and the hazard is a `Conflict` with a pid in it, or a
transcript whose tool results say which process has them. `~/.robi/tui.lock` is
deleted with the code that took it.
