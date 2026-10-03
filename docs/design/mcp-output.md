# MCP tool output

This page defines how a large MCP tool result reaches the model smaller, and
how the model gets the bounded text back. It is the MCP row of **M9** in the
[roadmap](../roadmap.md). Connecting a server, the 256 KiB bound, and the
content-block rendering stay in [mcp.md](mcp.md). The line collapser the
text pass calls stays in [shell-output.md](shell-output.md). The session
table stays in [persistence.md](persistence.md).

Turn compaction, which summarizes older messages when the window fills, is
[roadmap](../roadmap.md) F3.4. This page runs earlier, on one result, before
that result is appended.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Whether the server is called, and the 256 KiB bound on the joined text | [mcp.md](mcp.md) |
| Collapsing a shell log, and the `<<<ROBI_LOG` marker grammar | [shell-output.md](shell-output.md) |
| Crushing a `grep` result or a built-in JSON tool | `docs/design/tool-output-compression.md` |
| Folding a source file | [code-outline.md](code-outline.md) |
| Summarizing older turns | [roadmap](../roadmap.md) F3.4, `docs/design/context-management.md` |

## Problem

An MCP tool returns one string: the text blocks of `tools/call`, joined,
already cut at 256 KiB. `list_issues`, a GitHub search, and a SQL client
return a JSON array of the same object hundreds of times. A log-shaped
server returns the same line hundreds of times. The model pays for every
repeated field on later turns.

The shell collapser is the wrong store for that string. Stuffing the text
into `stdout` with an empty `stderr` and `exit_code: 0` invents a command
that did not run, and `retrieve` then describes the original as a shell
stream. Skipping every string that starts with `{` or `[` leaves the array
in the transcript whole, which is the result that is actually large.

A model call that summarizes the payload was rejected with the rest of M9.
The server's text is untrusted, and a summary spends a turn to do what a
walk of the JSON already does.

## Decision

Names that start with `mcp_` take one branch of the compressor the loop
already calls, after redaction and after the client's 256 KiB bound, and
before `enforce_result_ceiling`. The client does not crush the text. A
tool error never enters this branch: `isError` is an error string on the
tool call, and errors stay verbatim.

The result is a JSON string. Routing looks at that string:

| Shape | Pass |
|---|---|
| Parses as a JSON array or object | The JSON pass below |
| Anything else at least 4 KiB | The shell line collapser, then head, tail, and error windows when the collapse is still over 4 KiB |
| Under 4 KiB, or a pass that saves under 1 KiB | Byte-identical. No row |

A top-level JSON string, number, boolean, or null is dense. It takes the
line pass only when the raw text is at least 4 KiB, which a scalar almost
never is.

The original a later `retrieve` returns is this bounded string, including
the `[truncated at N bytes]` line when the client cut a tail. It is not
the rest of what the server sent, and it is not a shell stream.

```text
mcp execute
    |
    |  joined text already <= 256 KiB
    v
redact
    |
    v
under 4 KiB -----------------> unchanged, no row
    |
    parses as array or object -> JSON pass
    otherwise -----------------> line collapser, then slice
    |
    savings under 1 KiB -------> unchanged, no row
    |
    v
one tool_originals row, kind "mcp"
header line carries that row's id
    |
    v
transcript stores the compressed string
core ceiling still applies
    |
    v
tool card reads the row and shows the bounded text
```

The walk lives in `crates/robi/src/compress/mcp.rs`. It calls
`compress_stream` from the shell pass for the line shape. `robi-core` does
not parse JSON to decide a shape and does not open the database. A failing
insert returns the tool result unchanged and does not fail the turn.

## What is stored

One row per call that actually replaced the string. Same table as a shell
original. The blob is JSON:

```json
{
  "kind": "mcp",
  "text": "<bounded tool result>",
  "truncated": false
}
```

`truncated` is true when the client's bound appended `[truncated at N
bytes]`. `text` is the string the tool returned, redacted, including that
line. Deleting the session deletes the row.

A shell row stays `{"stdout","stderr","exit_code","truncated"}` and has no
`kind`. `retrieve` and the originals route tell the two apart by `kind`.

The transcript stores the compressed string only. Restart loads that
string, so a provider prompt-cache prefix is not rewritten, and `retrieve`
still resolves the id.

## JSON pass

Parse with `serde_json`. A string that only starts with `{` or `[` and does
not parse is the line pass. NDJSON and two values concatenated are the
line pass.

The rendered body is one JSON object the model can parse, after a header
line. The header is not part of that object.

### An array of objects

This is the common server payload.

Walk every element. An element that is not an object sends the whole array
to the generic array rule below.

Collect keys in the order they are first seen, at most 64. Past 64, the
object records `keys_omitted`. For each key, record the JSON types seen
(`string`, `number`, `boolean`, `null`, `object`, `array`).

Keep the first 3 elements as the sample. An element whose serialized form
is over 4 KiB is shrunk before it joins the sample: each array value
becomes `{"_items": N}`, each nested object becomes `{"_keys": ["..."]}`
with at most 32 keys, and each string over 200 characters becomes its
first 200 characters plus `…`. A sample that is still over 4 KiB after
that shrink is dropped from the sample, and `sample_omitted` counts it.

When at least 80% of elements have a string or number at `id`, then
`identifier`, then `name` — first key in that list that qualifies — the
remaining elements contribute that value. Keep the first 40. `rest_omitted`
is how many elements are neither in the sample nor in that id list.

```json
{
  "compressed": "mcp-json",
  "items": 240,
  "keys": ["id", "title", "state"],
  "types": {"id": ["string"], "title": ["string"], "state": ["string"]},
  "sample": [],
  "rest_ids": [],
  "rest_omitted": 197
}
```

`sample` holds the shrunk elements. `rest_ids` is omitted when no key
qualified. Key order in `keys` is first-seen order, not sorted.

### Any other array

Keep the first 8 elements, each shrunk by the same 4 KiB rule. `items` is
the full length. `omitted` is `items` minus how many elements were kept.
There is no `keys` and no `rest_ids`.

### An object

Walk top-level keys in order. A scalar, or a string of at most 200
characters, is copied. A longer string becomes `{"_chars": N, "_head":
"<first 200>"}`. An array uses the array rule in place and replaces the
value. A nested object is copied when its serialized form is at most 2
KiB; otherwise it becomes `{"_keys": ["..."]}` with at most 32 keys.
Deeper nesting inside that copy is left as parsed. The rendered object
also has `"compressed": "mcp-json"`.

### When the pass saves nothing

Render the object, prefix the header, and compare UTF-8 length with the
input. Under 1 KiB saved, the input string is returned and no row is
written. An empty array and a small object take this path because they
were already under 4 KiB, so the pass does not run.

## Line pass

`compress_stream` from the shell page, on the whole string. The same 4 KiB
gate and the same 1 KiB savings rule. The stored row is still `kind:
"mcp"` and `text`, not a shell body. The header uses `kind=mcp` and omits
`exit`.

Phase 2's error windows still pin `error:` and panic lines inside a
log-shaped server result. They do not run on the JSON pass.

## The header

The first line of a replaced string:

```text
<<<ROBI_LOG id="018f3b2c-7c1a-7a21-8c4e-9a21c4e8b0d1" sha256="99a21c4e8b0d1a22" kind=mcp>>>
```

`sha256` is the first 16 hex characters of the row hash, the same check
digit as a shell header. Lookup is by `id`. The JSON pass puts the
rendered object on the following lines. The line pass puts the collapsed
text there, including that pass's own `<<<ROBI_LOG repeated=…>>>` and
`omitted=` markers.

The tool description for every `mcp_` tool already says the result is
untrusted data. `retrieve`'s description gains one sentence: a header with
`kind=mcp` is the bounded text of that MCP call, not command output, and
`stream` does not apply.

## `retrieve`

Same tool. No new name.

When the row's `kind` is `mcp`, the result is `kind`, `text` paged with
the existing `offset`, `limit`, and `raw` rules, `truncated`,
`start_line`, `end_line`, `total_lines`, and `next_offset`. `stream` is
ignored. Reduced lines are the paging unit, as on a shell stream. `raw:
true` still returns at most 32 KB of `text`.

A row with no `kind` is a shell original and keeps today's fields.

`GET /api/v1/chat_sessions/{id}/tool_originals/{original_id}` returns the
same split. A shell row is `kind: "shell"`, `stdout`, `stderr`,
`exit_code`, `truncated`. An MCP row is `kind: "mcp"`, `text`,
`truncated`. The card for an `mcp_` call with `original_id` set loads that
route and shows `text`. A missing row shows the transcript string. The
turn does not fail.

## Rejected alternatives

- **A compressor inside the MCP client.** Already rejected in
  [mcp.md](mcp.md). The client would skip the session row, and a second
  caller would copy the walker.
- **Storing the text as a shell body.** `exit_code: 0` and an empty
  `stderr` are not properties of the call. `retrieve` would page a
  command that did not run. `kind` is the whole distinction.
- **A second retrieve tool.** The id space is one table. A second name
  teaches the model two ways back to one row.
- **Summarizing the JSON with the session model.** The payload is
  untrusted and often most of the turn. The walk is deterministic and
  makes no model call.
- **Pretty-printing before the pass.** The stored original is the string
  the tool returned. A re-serialize would be a different original than
  the one the server sent.
- **Crushing `isError` text.** The model needs the server's failure
  verbatim. Those calls do not enter the compressor.
- **A learned model for this shape.** The array pass is the saving. A
  local model is the same last phase shell output already deferred.

## Failure modes

- The string starts with `{` or `[` and does not parse. The line pass
  runs. A line pass that saves under 1 KiB stores nothing.
- The JSON walk renders a body that saves under 1 KiB. The transcript
  keeps the input string. No row.
- The insert fails. The transcript keeps the input string. The turn
  continues.
- `retrieve` is pointed at an MCP row with `stream` set. The text is
  returned and `stream` is ignored.
- The originals route is missing the row. The card shows the transcript
  string, header included.
- The client truncated at 256 KiB. `truncated` is true and `text` ends
  with the truncation line. `retrieve` cannot see past that cut.
- A header line is not server content. Editing tools already reject a
  `<<<ROBI_LOG` line in `old` or `new` when that line is not file text.
  The MCP result is not a file.

## Testing

`cargo test -p robi-core` stays free of the walker and the table.

`compress/mcp.rs` covers: an array of 200 objects keeps 3 samples, the
id list, and `items`; a 4 KiB nested object inside a sample becomes
`_keys`; a non-object array keeps 8 elements; a `{` prefix that does not
parse is not the JSON pass; a body that saves under 1 KiB returns none.

The compressor covers: a large `mcp_` string stores `kind: "mcp"` and
replaces the transcript value with a string whose first line is the
header; a string under 4 KiB is byte-identical and writes no row; an
insert error returns the input; a tool name that does not start with
`mcp_` is unchanged by this branch.

`retrieve` covers: paging an MCP row ignores `stream` and returns `text`;
a shell row still returns `stdout` and `stderr`.
