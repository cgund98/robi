# File attachments

A user can attach text files to one message. Each attachment has a display name
and an optional line range, and may come from inside or outside the workspace.
The composer and the transcript draw each one as a `filename (1-10)` chip.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Uploaded images | [persistence.md](../persistence/persistence.md), the image store |
| `@id` skill loads | [skills.md](../reach/skills.md) |
| The `read_file` tool and its path rules | [read-tools.md](../tools/read-tools.md) |
| The composer and transcript chrome | [chat-ui.md](chat-ui.md) |

## Problem

A user often wants the model to read a file, or part of one, without the model
spending a tool call to find it. The old paperclip read a text file in the
browser and folded it into the prompt string. That worked, but the transcript
bubble then carried the file's bytes, there was nothing to render as a chip, and
the model's input was not reconstructable from the message.

## Decision

**An attachment is a sibling field on the message.** `Message.files` is a
`Vec<FileAttachment>`, beside `skills` and `images`. Each entry is:

```json
{ "name": "error.rs", "path": "src/error.rs",
  "start_line": 29, "end_line": 34, "text": "…the slice…" }
```

- `text` is the attached slice: the whole file, or the selected range.
- `path` and the range are display metadata. `path` is `None` for an upload,
  which has no workspace location; the range is absent for a whole-file attach.
- `start_line` and `end_line` are 1-based and inclusive, matching `read_file`.

Because the text is stored on the message, a later request rebuilds the same
prompt from the transcript. Nothing re-reads the file at request time.

**The client reads the bytes; the server never opens `path`.** The composer
reads the file and sends the slice. The server measures the text the client sent
and never dereferences the path. That is the whole reason an attachment outside
the workspace is safe with no grant: there is no server-side read to confine.
An explicit attach is the consent.

**Never trust the client for size.** The handler re-checks the caps.

## Wire to the model

Each provider adapter appends one block per attachment to the text of the user
turn, in `user_text` (Anthropic) and `user_content` (OpenAI-compatible), right
after the skill blocks. `crates/robi/src/agent/files.rs` renders it:

```
<file name="error.rs" path="src/error.rs" lines="29-34">
…text…
</file>
```

`path` is omitted for an upload; `lines` is omitted for a whole-file attach. The
model reads the block as text, so it can widen the read with
`read_file(path, offset, limit)` — `offset` is `start_line`, `limit` is
`end_line - start_line + 1`.

## Transport

`POST /api/v1/chat_sessions/{id}/messages` takes a `files` array. A JSON body
carries it directly. A multipart body (used only when images are also present)
carries it as one `files` field whose value is the JSON array, because a
multipart part cannot hold a nested object.

The caps, enforced server-side in `build_files`:

| Cap | Value | Response |
|---|---|---|
| Files per message | 8 | `400` |
| One attachment's bytes | 64 KiB | `413` |
| All attachments' bytes | 256 KiB | `400` |

A file with a blank name is `400`.

`GET /chat_sessions/{id}/messages` returns `ChatMessage.files` as **metadata
only** — `name`, `path`, `start_line`, `end_line`. The attached text is not
returned, so a transcript read does not carry every attachment's bytes. The chip
is the affordance; the source file is still on disk.

## The chip

`src/components/chat/AttachmentChip.tsx` is the one widget, used in three
places: the composer (with a remove button), the transcript, and the pending
echo. It reads `filename (start-end)`, or just `filename` when there is no
range.

**The echo.** A message that has been sent but not yet stored shows as a pending
echo. The echo carries the attachment metadata (`PendingEcho.files`) so the chip
appears immediately, before the stored row exists.

**Where a range comes from.** The composer never sets one: the OS file picker and
a drop have no notion of a source line, so the composer only ever produces
whole-file attachments (`filename`, no range). A range is a producer concern. The
docs viewer is the built producer: hovering a rendered block in `#/docs` shows an
add-to-chat button, and clicking it slices that block's raw markdown lines
(`attachmentFromDocument` in `src/components/docs/docAttachment.ts`) and hands the
composer a ready `FileAttachment` with `start_line` and `end_line`. The block's
source range comes from `data-md-lines`, which `AssistantMarkdown`'s document mode
takes from the mdast `position` react-markdown carries on every element — the file
line, not a rendered one. See
[docs-viewer.md](docs-viewer.md#attach-a-line-to-chat).

The review screen is a second range producer, and it sends directly rather than
handing off to the composer. **Reject with reason** on a hunk builds one
attachment from that hunk's `new_start` / `new_count` range
(`src/components/review/reviewAttachment.ts`), and on a whole file builds a
whole-file attachment with no range. It decides the reject, returns to the chat,
and calls `sendInstruction` with the attachment. See
[code-review.md](../review/code-review.md).

The producer does not pass the attachment down as a prop, because attaching a
line opens the chat tray and the composer is not mounted until it does. It calls
`requestComposerAttachment(draftKey, file)` (`src/state/composerAttachments.ts`),
a keyed one-shot queue, and the composer drains it on mount and on each request.
The key is the composer's `draftKey` — the selected session id, or `draft` when no
session is selected, so a line attached with no chat open lands in a new chat. The
drained attachment goes through the same caps as a picker or a drop
(`appendAttachments`). A future code viewer on the M10 shared editor is the next
producer, and it uses the same handoff.

## Prefix caching

Storing the text **protects** the prefix cache. Prefix caching keys on a
byte-identical token prefix, so a frozen message never moves when the source
file changes on disk. The design that would invalidate the cache is the rejected
one — storing only a `path` and re-resolving at request time, where editing the
file changes an earlier turn's tokens and breaks the prefix from that point on.

The tradeoff is a straight swap: freeze keeps the cache stable but can go
**stale**. The transcript holds what the user attached, not what the file says
now. If the file changes, the model reasons about the old bytes until it calls
`read_file`.

## The paperclip

The composer's paperclip keeps its image/text split. Images are unchanged: they
go out as `ImageAttachment` and the bytes live in the image store. Anything that
is not an image becomes a `FileAttachment` through the path above. The old
`instructionWithTextFiles` fold is gone; the size and NUL checks moved into the
attachment builder.

**What counts as text is decided from content, on the server.** The client sends
each file's raw bytes, base64-encoded, as `content_base64`; the server
base64-decodes them and applies the same rule the read tools use (`read_code`,
`grep`): a NUL byte marks binary, and the bytes must be valid UTF-8. Anything
else is `415`. No file name or extension is consulted, so `.log`, `.conf`, and
extensionless files are text when their bytes are. The client sniffs the same
rule (`looksLikeText`) for feedback at attach time, but the server is the
authority and re-checks.

Also on attach: a file over 64 KB per file or 256 KB total is refused, and the
server re-checks both caps after decoding (an oversized base64 body is rejected
before decode). A blank name is `400`; invalid base64 is `400`.

**In-workspace vs outside is also decided on the server.** The client sends the
file's `absolute_path` when its picker provides one; the server canonicalizes it
and the session's workspace root, and a path under the root becomes the stored
workspace-relative `path` (so the model can re-read it with `read_file`). A path
that is outside, or that cannot be resolved because the server cannot see it,
keeps only its `name` and is treated as outside; the provider block then carries
`origin="outside-workspace"`. The server canonicalizes to compare — that
resolves symlinks, so a link out of the workspace is outside — but it never reads
the file's contents.

The absolute path only exists in the desktop app: the paperclip opens the OS
dialog through a Tauri command (`pick_attachment_files`) that returns each file's
absolute path and bytes. The webview's `<input type="file">` exposes only the
basename, so a browser upload — or any drop or paste — has no `absolute_path` and
is treated as outside. `path` is never stored for an outside file and is never
opened by the server.

## Rejected alternatives

- **Fold the text into `content`.** The old paperclip. The bubble then carries
  the bytes, the chip needs separate metadata anyway, and the model's input is
  no longer a field. Rejected.
- **Store only `path` + range and re-resolve at request time.** Always fresh, but
  it invalidates the provider prefix cache on every file edit and routes a hidden
  server-side read of a user-named path. Rejected.
- **Keep the fold beside the new system.** Two paths for one thing, two
  renderings. Rejected: one path.

## Failure modes

- A file over a cap is refused before the message is stored; the composer shows
  the error.
- A file that is not UTF-8, or that contains a NUL, is refused by the client
  before send.
- A missing `files` field deserializes as an empty list, so an older client and
  an older stored row both load.
