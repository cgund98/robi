# Shell output

This page defines how a large `shell` result reaches the model smaller, and
how the model gets the capped stdout and stderr back. It is the log row of
**M9** in the [roadmap](../../roadmap.md). The command, the sandbox, and the
256 KiB stream cap stay in [shell-tool.md](../tools/shell-tool.md). The session
database that holds the original stays in [persistence.md](../persistence/persistence.md).
Source outlines are a different tool, in [code-outline.md](../tools/code-outline.md).

JSON arrays and search hits are not this page. They remain
`docs/src/design/compression/tool-output-compression.md`.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Whether the command runs, and the 256 KiB cap on each stream | [shell-tool.md](../tools/shell-tool.md) |
| Folding a source file | [code-outline.md](../tools/code-outline.md) |
| Which MCP result uses this collapser | [mcp-output.md](mcp-output.md). The line pass calls this collapser. JSON takes that page's own pass |
| Crushing JSON or a `grep` result | `docs/src/design/compression/tool-output-compression.md` |
| The context meter drawing the saving | [roadmap](../../roadmap.md) F3.4. This page says the compressor reports bytes in and bytes out |

## Problem

`cargo test`, `pnpm test`, and a failed build write the same line shape
hundreds of times, then put the useful part in the exit code, one summary
line, and a short diagnostic. The model pays for every repeated `... ok`
on later turns. Cutting the stream at 256 KiB keeps a prefix of setup and
drops the summary that landed at the end.

A substring search for `"error"` is not a compressor. It keeps lines that
merely contain the letters, it repeats the head inside every window, and it
still has no copy of the bytes it dropped. A passing suite and a failing
build need different cuts. A learned model is a third cut, and it is the
expensive one.

## Build order

Four phases, cheapest useful cut first. The store is not the last piece of
work. A collapsed view with no way back is the silent truncate M9 forbids,
so the store ships in the same change as phase 1.

| Order | Phase | What it costs | What it saves |
|---|---|---|---|
| 1 | Line collapsing | A state machine over lines. No I/O | A green `cargo test` or `Compiling` run, often down to a handful of lines |
| 2 | Head, tail, and error windows | Range merge on the collapsed lines | A failed build whose middle is still large after collapsing |
| 3 | Session store and `retrieve` | One table, written beside phase 1 | Nothing by itself. It is what makes 1 and 2 safe to show the model |
| 4 | A local line model | An ONNX runtime, feature-gated | Only the residue phases 1 and 2 still leave. Measure that residue before writing this |

Phases 1 and 2 are pure functions of one `&str`. They live in
`crates/robi/src/agent/compress/shell.rs`. `robi-core` does not take a `regex`
dependency and does not open the database. The loop calls a compressor
trait. The shell implementation and the table live in `crates/robi`. A
failing compressor returns the tool result unchanged and does not fail the
turn.

`shell` does not compress its own result. The pipeline below runs for a
`shell` result, on both streams. Line-oriented MCP text calls the same
collapser; which MCP shapes do that, and how the original is stored, is
in [mcp-output.md](mcp-output.md). Every other tool is unchanged until
its own compressor exists.

## Pipeline

Compression runs after `Tool::execute` and after the shell's own cap, and
before `enforce_result_ceiling`. Each of stdout and stderr is already capped
at `max_tool_result_bytes` (256 KiB) inside the launcher. The original a
later `retrieve` returns is those capped strings, including `truncated` when
the launcher dropped a tail. It is not the rest of the pipe.

```text
shell execute
    |
    |  each stream already <= 256 KiB
    v
redact the tool result
    |
    v
per stream:
    under 4 KiB ---------> unchanged, no row
    |
    phase 1 collapse runs
    phase 2 slice, only if the collapse is still over 4 KiB
    savings under 1 KiB -> unchanged, no row
    |
    v
one tool_originals row for the call (both capped streams)
marker lines carry that row's id
    |
    v
transcript stores the compressed result
core ceiling still applies to that JSON
    |
    v
tool card reads the row and shows the capped streams
```

The trait the loop calls:

```rust
pub struct CompressRequest<'a> {
    pub tool: &'a str,
    pub result: &'a serde_json::Value,
}

pub struct CompressOutcome {
    /// Replaces `result` in the transcript. Equal to the input when this
    /// call was a no-op.
    pub result: serde_json::Value,
    pub original_id: Option<String>,
    pub original_bytes: Option<Vec<u8>>,
}
```

`original_bytes` is the JSON object `{"stdout","stderr","exit_code","truncated"}`
from the tool result, before either stream was replaced. The core records
`original_id` on the tool call and does not write the blob. The `crates/robi`
implementation writes the blob in `compress`, or returns the bytes and lets
the adapter write them in the same transaction as the transcript append.
Either way `robi-core` does not name SQLite. Tests pass a compressor that
returns the input and `original_id: None`.

A stream shorter than 4 KiB is returned byte-identical. Phase 1 still runs
on a longer stream. If the rendered stream is not at least 1 KiB smaller
than the input, the stream is returned byte-identical and no row is written.
A call where both streams take that path stores nothing.

## Phase 1 — collapsing repeated lines

This is the whole compressor for a green test run. Phase 2 does not run
when phase 1 already fits.

### Preparing lines

Split on `\n`. Inside each line, if the line contains `\r`, keep only the
segment after the last `\r`. A progress bar is one line, not the history of
the bar. The stored original keeps the raw stream, carriage returns
included. Line numbers in markers count these reduced lines, and `retrieve`
documents that its `offset` counts the same way: split on `\n`, then last
`\r` segment. A caller that needs the raw bytes uses `retrieve` with
`raw: true` (phase 3).

A line is pinned when it matches an error pattern from phase 2. A pinned
line is never absorbed into a run. It is emitted as itself. That is what
keeps `test tests::auth ... FAILED` out of a passing collapse.

### Runs

Walk the lines once. A run is contiguous lines with the same kind and the
same template. A pinned line, a blank line, and a kind change all flush the
open run.

| Kind | Match | Template | Flush at |
|---|---|---|---|
| `cargo-test` | `^test \S.* \.\.\. (ok\|ignored)$` | The status word only. Names may differ | 3 lines |
| `cargo-progress` | `^\s*(Compiling\|Checking\|Downloading\|Downloaded\|Documenting\|Finished)\b` | The verb | 3 lines |
| `generic` | Anything else unpinned | Numerics, hex, and UUIDs replaced, below | 4 lines |

`FAILED` is not in the cargo-test kind. Those lines are pinned by the error
table, so a failing test breaks the run and stays verbatim.

The generic template is one compiled regex, not a fresh pattern per line:

```rust
static SLOT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        [0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}
        | \b[0-9a-fA-F]{8,}\b
        | \b\d+(?:\.\d+)?(?:ms|s|us|µs|KiB|MiB|GiB|KB|MB)?\b
        ",
    )
    .expect("slot regex")
});

fn template_of(line: &str) -> String {
    SLOT.replace_all(line.trim_end(), "0").into_owned()
}
```

`test tests::test_auth_1 ... ok` and `test tests::test_auth_2 ... ok` share
the cargo-test kind and the status `ok`, so they are one run even though
the names differ. `Downloading crate A v1.0.0` and `Downloading crate B
v1.2.3` share the verb. Two unrelated lines share a generic template only
when they match after slots are replaced, for example the same sentence
with a different count. A digit run that sits against a letter or `_`
(`v1.2.3`, `test_auth_1`) is not a slot: `\b` does not split it. Those
lines collapse only when a kind above already grouped them.

The walk keeps the first line, the last line, the count, and the original
line range. It does not allocate a `String` per line of a generic run. A
cargo-test run also keeps the captured name of the first line and of the
last line, which is enough for the star pattern below. Names are the `\S.*`
between `test ` and ` ... `.

```rust
struct OpenRun {
    kind: Kind,
    template: String,
    first: String,
    last: String,
    first_name: String,
    last_name: String,
    start: usize, // 1-based reduced line
    end: usize,
    count: usize,
    /// Present only while `count` is still under the flush threshold.
    buffered: Vec<String>,
}
```

A short run is copied through verbatim from `buffered`. Once `count`
crosses the threshold the buffer is cleared and the run keeps the two
endpoint lines plus the count. Flushing a long run writes the summary and
a `<<<ROBI_LOG repeated=… lines=…>>>` marker. Flushing a short run writes
`buffered`. A line is never discarded because a run ended.

### What a run becomes

Cargo test, shared name prefix at least 8 bytes and ending on `::` or `_`:

```text
test tests::test_auth_* (48 tests passed) ... ok
<<<ROBI_LOG repeated=48 lines=20-67>>>
```

`ignored` uses `ignored` instead of `passed`. The prefix is the shared
bytes of `first_name` and `last_name` only. That is a sample of the run,
not a proof that every middle name shared the prefix. When the two ends do
not share such a prefix, the star is the whole name:

```text
test * (48 tests passed) ... ok
<<<ROBI_LOG repeated=48 lines=20-67>>>
```

Cargo progress:

```text
Compiling ... (36 crates)
<<<ROBI_LOG repeated=36 lines=4-39>>>
```

The first line of the run is not repeated beside that summary. The summary
is the verb from the template (`Compiling`, `Checking`, `Downloading`,
`Downloaded`, `Documenting`, `Finished`) plus the count. `Finished` rarely
repeats; the threshold of 3 leaves a normal single `Finished` line alone.

Generic:

```text
<<<ROBI_LOG repeated=12 lines=40-51>>>
    <first line, truncated to 200 bytes on a char boundary>
```

The first line is real output, so the model can see the shape. The other
eleven are not copied.

`<<<ROBI_LOG` is the marker prefix. It is not a comment and it is not
source. `edit_file` and `write_file` reject `<<<ROBI_` in `old`, `new`, and
`content`, as [code-outline.md](../tools/code-outline.md) specifies.

A green `cargo test` of a few hundred tests becomes the progress summaries,
one test summary, and the real `test result:` line. That result is under
4 KiB. Phase 2 does not look at it.

## Phase 2 — head, tail, and error windows

Run this only when phase 1's text is still over 4 KiB. The input is the
phase 1 text, whose collapse lines and pinned error lines are already one
each. Line numbers in the markers are translated back to the reduced
original through the ranges phase 1 recorded. A marker never cites a line
number in the collapsed text.

Pinned events, which phase 2 must not drop:

- Every phase 1 summary line.
- Every line that matched the error table, and the window around it.

Everything else is eligible to be omitted outside the head and the tail.

Keep the first 12 reduced lines and the last 20. On a stream shorter than
32 lines those two ranges cover it, so phase 2 returns the phase 1 text.

### Error lines

Match per line, anchored. Do not search for the substring `error`,
`failed`, or `panic`. Those match `test tests::error_path ... ok`,
`0 errors`, and `panic_handler`.

| Pattern | Window after the hit |
|---|---|
| `^error(\[E\d+\])?:` | 2 lines |
| `^Error:` | 2 lines |
| `^panic:` or `panicked at` | 40 lines |
| `^stack backtrace:` | 40 lines |
| `^Traceback \(most recent call last\):` | 40 lines |
| `^thread '.*' panicked` | 40 lines |
| ` \.\.\. FAILED$` | 2 lines |
| `^FAIL\b` | 2 lines |
| `^npm ERR!` | 2 lines |
| `^error TS\d+:` | 2 lines |
| `^E\s{2,}\S` | 2 lines |

The window is 2 lines before the hit and the after-count above, clamped to
the stream. A 40-line stack window is how a panic stays intact. A 2-line
window around `error[E0308]:` keeps the rustc span lines that follow it.
Overlapping windows merge into one range.

`warning:` is absent on purpose. A build can emit hundreds of them. They
collapse in phase 1 when they share a template, and phase 2 may omit the
rest. The first warnings that sit in the head are still there.

At most 200 lines of error windows are kept, earliest hits first. A log
that is 400 panics keeps the first windows and the tail. The omission
marker says how many error hits were not given a window. The tail is still
kept.

### Rendering a gap

Walk the merged keep-ranges in order. Copy the kept text. Between two
ranges, one marker:

```text
<<<ROBI_LOG omitted=4200 lines=120-4319>>>
```

`omitted` is the number of reduced original lines in the gap. `lines` is
the 1-based inclusive range in that same numbering. Adjacent gaps do not
occur: merging the keep-ranges makes every gap one span.

Head and tail that overlap become one range. No marker is inserted in an
empty gap. The sketch that always pushes `"... [middle logs omitted] ..."`
is not this renderer.

Exit code stays the `exit_code` field on the tool result. Phase 2 does not
parse a summary and does not invent `Passed: 142`. When the real
`test result:` line is in the tail, it is copied because it is in the last
20 lines. When a non-zero exit code's stderr is under 4 KiB, phase 2 never
sees it and the model gets that stderr whole.

## Phase 3 — the original, and retrieve

This ships with phase 1. It is listed third because it does not shrink a
log. It is the reason a shrunk log is allowed into the transcript.

### What is stored

One row per shell call that actually replaced a stream. The blob is the
capped stdout, the capped stderr, the exit code, and the tool's `truncated`
flag, as JSON. Redaction has already run. Deleting the session deletes the
row.

The transcript stores the compressed result only. The tool card loads the
row and renders the capped streams, which is the output the user already
sees today. A missing row shows the transcript text. The turn does not
fail. Restart loads the transcript as stored, so a provider prompt-cache
prefix is not rewritten, and `retrieve` still resolves the id.

`robi-core` records the id on the tool call. The bytes go in the session
database:

```sql
CREATE TABLE tool_originals (
  id TEXT PRIMARY KEY,
  chat_session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
  tool_call_id TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX tool_originals_session ON tool_originals (chat_session_id, sha256);
```

`id` is a UUIDv7, the same shape as a message id. `sha256` is the hex
SHA-256 of `body`. `body` is the JSON above, not a second compression.
`tool_call_id` is the call whose result was replaced.

The logical key-value record, which is what a `redb` table would have been,
is `(session_id, id) -> body`. The session database is that map. A `redb`
file at `~/.robi/logs.redb`, keyed by `session_id || id` with the body as
the value, duplicates a database the process already opens and does not
cascade when the session row is deleted. `sled` is the same split. Neither
crate is added.

### The marker the model sees

The first line of a stream that was changed is a header. Streams that were
left byte-identical do not get one.

```text
<<<ROBI_LOG id="018f3b2c-7c1a-7a21-8c4e-9a21c4e8b0d1" sha256="99a21c4e8b0d1a22" exit=1>>>
test tests::test_auth_* (48 tests passed) ... ok
<<<ROBI_LOG repeated=48 lines=20-67>>>
test tests::auth ... FAILED
thread 'tests::auth' panicked at src/auth.rs:88:5:
assertion `left == right` failed
<<<ROBI_LOG omitted=1200 lines=40-1239>>>
test result: FAILED. 142 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.02s
```

`sha256` in the header is the first 8 bytes of the row's hash, 16 hex
characters. It is a check digit. Lookup is by `id`.

### `retrieve`

The tool name is `retrieve`. It is `Concurrent`. It reads one row in the
current session.

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": ["id"],
  "properties": {
    "id": {
      "type": "string",
      "description": "The id from a <<<ROBI_LOG header, or the 16-hex sha256 from that header when it matches one row in this session."
    },
    "stream": {
      "type": "string",
      "enum": ["stdout", "stderr", "both"],
      "default": "both",
      "description": "Which capped stream to return. both returns stdout, then stderr, with a stderr header line between them when stderr is non-empty."
    },
    "offset": {
      "type": "integer",
      "minimum": 1,
      "description": "1-based reduced line to start at. Reduced means split on newline, then the segment after the last carriage return. Omit to start at line 1."
    },
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 200,
      "default": 200,
      "description": "Maximum reduced lines to return."
    },
    "raw": {
      "type": "boolean",
      "default": false,
      "description": "True: return the stored stream bytes unchanged, still capped at 32 KB for this call, ignoring offset and limit. False: reduced lines."
    }
  }
}
```

The result is `stdout` and `stderr` (either may be empty when `stream`
selects one), `exit_code`, `truncated` from the stored tool result,
`start_line`, `end_line`, `total_lines`, and `next_offset` when the page
is not the end. `total_lines` counts reduced lines.

A 16-hex id that matches two rows in the session is `ambiguous id`, and the
error lists the full ids. A missing id is `original not found`. An id from
another session is the same error. The tool does not search other sessions.

`raw: true` exists so a progress bar can be inspected. The call still
returns at most 32 KB, the same bound as `read_file`, with `truncated` and
a byte `next_offset` on the stored body. It does not pull the entire 256
KiB stream back into the transcript in one call. The model pages.

The description tells the model: the `<<<ROBI_LOG` lines are not command
output; call `retrieve` with the header id when a folded range or a stack
past the kept window is the thing you need; pass `offset` from the marker's
`lines`.

## Phase 4 — a local line model

Not in the first cut. Phases 1 and 2 are the compressors M9's exit criteria
ask for. This phase exists so a learned model is a measured addition rather
than the implementation of "drop boilerplate."

Headroom's Kompress model is the reference for the behavior, not a
dependency. The semantic-search embedder is the wrong model: it maps a
chunk to 768 dimensions for retrieval, and loading it on a tool result
couples log compression to the index feature.

What would be built, after a corpus of real shell results shows that phase
2 still leaves more than 8 KiB of low-value lines on a typical failed
build:

- A line classifier, ONNX, CPU, two intra-op threads, weights under
  `~/.robi/models/`. Feature `log-model`, off by default, same shape as
  `local-embed` on `robi-index`.
- It scores lines that phase 2 is about to omit. It does not override a
  pinned line, a summary, the head, or the tail. It may move a high-scoring
  omitted line into the kept set, with the same 2-line window, until the
  200-line middle cap.
- It never runs on a stream under 4 KiB, and never on a stream phase 1
  already shrank under 4 KiB.
- Weights missing, runtime missing, or a scoring error: the phase 2 text
  stands. The turn is not failed. `robi-core` does not link the runtime.

Shannon entropy of the bytes is not this phase. A repeated `... ok` line
and a unique short identifier can share a low score, and a minified or
base64 dump scores as high as a stack trace. The error table already keeps
the stack. An entropy threshold would be a second, less predictable copy of
phase 1.

## Rejected alternatives

- **Compressing inside `shell`.** Every tool would grow its own crusher.
  MCP results would miss it. The trait runs once, after execute.
- **The 100-line sketch that searches for `error`, `failed`, and `panic`.**
  `test tests::error_path ... ok` is kept as a diagnostic. Head lines are
  appended again inside each window. The middle marker is pushed even when
  the head and the tail already cover the stream. `exit_code` is unused.
  Nothing stores the dropped lines.
- **Collapsing `... FAILED` into the passing run.** The failure is the
  line the model needs verbatim, including the test name.
- **Slicing before collapsing.** The 12-line head is spent on `Compiling`
  lines that phase 1 would have turned into one summary.
- **Truncating a kept stack at 2 lines.** `panicked at` without the frames
  is how the model guesses the wrong frame. Stack-shaped hits keep 40
  lines.
- **Inventing `Passed` and `Failed` counts.** A wrong parse of the summary
  is worse than copying the real `test result:` line from the tail.
- **Storing the body in `redb` or `sled`.** The record is
  `(session, id) -> body`. The session database already cascades that on
  delete. A second file does not.
- **A `retrieve_raw_output` tool.** It is `retrieve` with an id. A second
  name is a second schema for one read.
- **Putting the original only in the transcript, and also sending it to
  the model.** The model then pays for the full stream, which is the
  problem. The card reads the row. The model reads the compressed result.
- **Recovering bytes the 256 KiB cap already dropped.** Those bytes were
  never the tool result. `truncated` on the stored object says so.
- **Running the line model in phase 1.** It is a large optional dependency,
  it is not deterministic, and the structural passes are the ones that
  shrink a test log. Measure a residue first.

## Failure modes

- A stream under 4 KiB is byte-identical. No row. No marker.
- Phase 1 and phase 2 together save less than 1 KiB: the stream is
  byte-identical. No row.
- A regex that fails to compile is a programming error. The patterns are
  constants. If `compress` returns an error anyway, the loop keeps the tool
  result and records no id.
- The database write fails: the loop keeps the uncompressed tool result.
  The model does not see a marker whose id will not resolve.
- `retrieve` with an unknown id, or an id from another session:
  `original not found`.
- `retrieve` with a 16-hex prefix that matches two rows in this session:
  `ambiguous id`, and the full ids.
- `offset` past the end: empty page, `total_lines` set, no error.
- The launcher set `truncated` because a stream passed 256 KiB. The stored
  body is that prefix. The compressed view still says the tool result was
  truncated. `retrieve` cannot return the dropped tail.
- The tool card cannot load the row. It shows the transcript text.
- Cancellation before the row is committed leaves no marker in the
  transcript, because the result is not appended either.
- Phase 4 weights or runtime fail: the phase 2 text is the result. The
  turn succeeds.
