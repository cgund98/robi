# Code outline

This page defines targeted AST unfolding: a source file reaches the model as
signatures plus the bodies it named, and an edit still has to quote bytes that
are in the file. The tool is `read_code`. Exact reads stay on `read_file`.
Exact writes stay on `edit_file` and `write_file`.

Symbol strings are the same strings the index stores. Chunking is in
[semantic-search.md](semantic-search.md). Path rules are in
[read-tools.md](read-tools.md). The edit match is in
[editing-tools.md](editing-tools.md). M9 compression is a different pipeline
and does not fold source. See
[tool-output-compression.md](tool-output-compression.md).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Line windows of exact source | [read-tools.md](read-tools.md). `read_file` is unchanged |
| Fuzzy or patch edits | [editing-tools.md](editing-tools.md). Matching stays exact |
| Embeddings and which node kinds become chunks | [semantic-search.md](semantic-search.md) |
| Crushing JSON, logs, and search hits | M9. An outline is a tool result the model asked for, not a compressor |
| Collapsing shell stdout and stderr | [shell-output.md](shell-output.md) |

## Problem

A 1,000-line file sent in full is mostly bodies the model is not editing.
Those lines are paid for on every later turn that still holds the result, and
they crowd out the signature the model actually needed. The cost is linear in
the file and in how many times the transcript repeats it.

Three common substitutes fail in specific ways:

| Approach | What the model receives | What goes wrong |
|---|---|---|
| Full file | Every byte, up to the 32 KB `read_file` cap, then another window | The cap splits a function in the middle. The next window is another tool call. A file under the cap still spends its whole body |
| Naive truncation | A prefix, a tail, or a character budget | The cut is not a syntactic boundary. The model invents the missing half or edits a signature whose body it did not see |
| A file-level compression flag | One switch for the whole file | Off dumps the file. On hides every body, including the one about to be edited. The flag cannot name `process_card` and leave `validate_expiry` folded |

Targeted unfolding is a view built from tree-sitter ranges. The signature of
every foldable item is copied from the source. The body is copied only when
the item is in `focus_symbols` or is an ancestor the focused item sits inside.
Every other body is replaced by one marker line that names the symbol, the
source line range, and a hash of the omitted bytes. The file on disk is not
modified. The marker is not source.

`read_file` of `src/pay.rs` returns the impl and both bodies. `read_code` with
`focus_symbols: ["PaymentService::process_card"]` returns the impl frame, the
full `process_card` body, and one marker where `validate_expiry` was:

```rust
impl PaymentService {
    pub fn process_card(&self, card: &Card) -> Result<Receipt, PayError> {
        let charge = self.gateway.charge(card)?;
        self.ledger.append(charge);
        Ok(Receipt::from(charge))
    }

    fn validate_expiry(&self, card: &Card) -> Result<(), PayError>
    <<<ROBI_OMITTED symbol="PaymentService::validate_expiry" lines=9-12 sha256="a1b2c3d4e5f67890">>>
}
```

The bytes of `process_card`, including its braces and indentation, are a
slice of the file. The marker is synthesized. `edit_file` may quote the
`process_card` lines. It may not quote the marker.

## Pipeline

`read_code` runs in `crates/robi/src/tools/read_code.rs`. The fold is a pure
function, `outline`, in `crates/robi-index/src/outline.rs`, next to the
chunker. It takes the source `&str`, the language, the focus list, `depth`,
and `expand_imports`. It returns the rendered string, the omitted spans, and
the match report. It does not read the filesystem and does not open the index
database. The tool does the I/O, then calls `outline`.

`robi-core` does not gain tree-sitter. The loop sees an ordinary tool result.

```text
read_code args
    |
    |  workspace::resolve_path
    |  PathFilter::allows_read
    v
one buffer: String  (UTF-8 file, at most 1 MiB)
    |
    |  compress == false
    |      `--> read_file's read_window  -->  view:"source"   (no parser)
    |
    |  compress == true
    v
Parser::parse(&[u8])  -->  Tree of byte offsets into that buffer
    |
    v
queries capture @body ranges; symbol_of names each node
    |
    v
select non-overlapping body ranges to omit
    |
    v
render: copy kept slices, write one marker per omitted range
    |
    v
if result > 32 KB: drop back one depth, then refuse a focused body
    |
    v
view:"outline"   (transcript stores this text, not a second body store)
```

The source buffer is the only copy of the file. The tree holds offsets, not
strings. A kept region is written once, from `&source[start..end]`, into the
output `String`. An omitted region is never copied into the output. There is
no per-node `String` and no `to_owned` of a body that will be thrown away.
The output itself is a new allocation: the skeleton is not a subslice of the
file, because markers are not in the file.

`mmap` is the wrong way to chase that. The file can change while the tree is
alive, and `read_file` already owns a `String`. One read into that `String`
is the rule. Tree-sitter's `parse` borrows it and does not take ownership.

```text
source: String
|-- bytes -----------------------------------------------|
   ^signature^  ^body kept^  ^sig^ XXX marker XXX  ^tail^
        |            |          |                      |
        +------------+----------+----------------------+
                     one write into out: String
Tree { nodes: Range<usize> only }
```

Order inside the tool:

1. Resolve the path and apply the session read filter, the same way
   `read_file` does. A denied path returns `path is not allowed` and no
   bytes.
2. Read the file. Missing, directory, and non-UTF-8 use the same errors as
   `read_file`. A NUL in the first 8 KB is `file is binary`. Larger than
   1 MiB is `file is too large; use read_file with offset and limit`. The
   parse never sees a larger buffer.
3. An extension with no grammar uses the `compress: false` path even when
   `compress` is true, and the result says `view: "source"` and
   `reason: "no grammar"`. The model gets a normal window, not a fake outline.
4. When `compress` is false, skip the parser. Call the same window helper as
   `read_file` (`offset`, `limit`, 32 KB). `view` is `"source"`.
5. When `compress` is true, parse. `root.has_error()` does not produce an
   outline. The result is a tool error: `file did not parse; use read_file`.
   A skeleton that skipped a broken function would hide the break.
6. Render. Apply the 32 KB output cap as specified under the size gate.
7. Return. The tool is `Concurrent`.

M9 runs after the tool returns, on the bounded result. An outline is already
the small view. The source-row rule still applies: this result is passed
through. A second compressor must not rewrite marker lines.

## Tool

The model already has `read_file`. `read_code` is a second tool because its
result is not a quote source. Mixing both shapes behind one name is how a
marker lands in `old`.

`compress` defaults to true. The schema default is not "true during search,
false during an edit." The tool cannot see intent. The description tells the
model which value to send. `focus_symbols` with `compress: false` is
`invalid arguments`, so a call cannot claim a full file and also name symbols
that were the only bodies returned.

`offset` and `limit` apply only when `compress` is false. When `compress` is
true they are `invalid arguments`. An outline is not a line window. Cutting
one in half splits a marker or a focused body.

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": {
    "path": {
      "type": "string",
      "description": "File to read. Workspace-relative, absolute, or starting with ~/."
    },
    "compress": {
      "type": "boolean",
      "default": true,
      "description": "True: outline. Signatures are source slices; bodies not in focus_symbols are markers. False: exact source, same 32 KB window as read_file, no markers. Omit this, or leave it true, unless you need bytes to quote in edit_file."
    },
    "focus_symbols": {
      "type": "array",
      "maxItems": 8,
      "items": { "type": "string", "minLength": 1, "maxLength": 200 },
      "description": "Qualified symbols whose bodies are copied in full. Rust and Go use '::'. Other languages use '.'. Example: [\"PaymentService::process_card\", \"validate_expiry\"]. A name that hits more than one symbol unfolds none of those hits. Ignored unless compress is true; sending it with compress false is an error."
    },
    "expand_imports": {
      "type": "boolean",
      "default": false,
      "description": "False: a leading import or use run longer than 15 lines becomes one marker. False does not fold a later import. True: that run is copied through."
    },
    "depth": {
      "type": "integer",
      "minimum": 0,
      "maximum": 2,
      "default": 1,
      "description": "How much of an unfocused file stays open. 0: top-level items collapse to a signature plus one marker, including impls and classes. 1: impls, classes, traits, and modules stay open; function and method bodies collapse; struct and enum field lists collapse. 2: top-level function bodies stay open too; only nested function bodies collapse. A focused symbol is expanded in full at every depth, and containers on the path to it stay open. Ignored when compress is false."
    }
  }
}
```

Eight focused symbols is the cap. More than eight is `invalid arguments`.
Naming every symbol in the file is a full read; that is `compress: false` or
`read_file`.

Outline result:

| Field | Value |
|---|---|
| `view` | `"outline"` |
| `path` | Workspace-relative path |
| `language` | The grammar name (`rust`, `typescript`, `tsx`, `javascript`, `python`, `go`) |
| `file_sha256` | SHA-256 of the file bytes, hex |
| `content` | The skeleton |
| `total_lines` | Lines in the file |
| `bytes` | Byte length of `content` |
| `truncated` | True only when the size gate could not keep the focused bodies |
| `depth_applied` | The depth that was rendered. Lower than requested when the size gate stepped down |
| `focused` | Symbols whose bodies were copied |
| `omitted` | `{symbol, start_line, end_line, sha256}` for each marker |
| `missing` | Focus strings that matched nothing |
| `ambiguous` | Focus strings that matched more than one symbol, each with the candidate symbols |

A missing or ambiguous name does not fail the call. The outline still
returns, and those arrays say what was not opened.

Source result, when `compress` is false or there is no grammar: the
`read_file` fields (`content`, `start_line`, `end_line`, `total_lines`,
`truncated`, `next_offset`) plus `view: "source"`. No `omitted`. No marker.

### What the model is told

The tool `description` is the instruction. The parameters schema above is the
rest. The built-in prompt lists the description with the other tools; it does
not grow a second copy of this policy.

```text
read_code returns an outline of a source file. Signatures are real source.
A body is real source only when you pass it in focus_symbols. Every other
body is one <<<ROBI_OMITTED ...>>> line. That line is not in the file.

Use compress true (the default) to see a large file. Pass focus_symbols for
the function, method, or struct you are about to reason about or edit.
Symbols use the same names as semantic_search: PaymentService::process_card
in Rust and Go, PaymentService.process_card in TypeScript and Python. A
bare name is accepted when it matches one symbol in the file.

Quote edit_file's old and new only from read_file, or from a read_code call
with compress false. Never copy a ROBI_OMITTED line into old, new, or
write_file content. To edit a folded body, call read_code again with that
symbol in focus_symbols, or read_file with offset set to the marker's
start line. depth 0 is the file map. depth 1 is the default. depth 2 keeps
top-level function bodies and is rarely worth it. expand_imports true only
when the bug is in the imports.
```

## Folding

Grammars are the ones `robi-index` already links: `tree-sitter-rust`,
`tree-sitter-typescript` (including TSX), `tree-sitter-javascript`,
`tree-sitter-python`, `tree-sitter-go`. Markdown is not outlined. A `.md`
file takes the source window.

Names come from the chunker's `symbol_of`, not from a second walk. A focus
string is the index symbol, so a `semantic_search` hit can be passed through
unchanged. Rust and Go join with `::`. The others join with `.`.

The body to replace is the grammar field `body`, not the whole item. The
bytes before `body.start_byte` stay. That is the signature, including
parameters, return type, and the newline that precedes the body. The bytes
after `body.end_byte` stay. For a Rust function the body field is the
`block`, braces included, so the outline does not keep a `{` with nothing
inside it.

Queries exist to bind that field per language. The policy in the next
section decides which of those ranges are replaced. A query match is not
itself a decision to omit.

Rust:

```scheme
(function_item
  name: (_) @name
  body: (_) @body) @item

(function_signature_item
  name: (_) @name) @item

(struct_item
  name: (_) @name
  body: (_) @body) @item

(enum_item
  name: (_) @name
  body: (_) @body) @item

(trait_item
  name: (_) @name
  body: (_) @body) @item

(impl_item
  body: (_) @body) @item

(mod_item
  name: (_) @name
  body: (_) @body) @item

(const_item name: (_) @name) @item
(static_item name: (_) @name) @item
```

`body: (_)` is deliberate. A Rust struct body is a `field_declaration_list` or an `ordered_field_declaration_list` (tuple structs). Naming one of those drops the other. The field name `body` is the contract; the node kind is not. `const_item` and `static_item` have a `value`, not a `body`, so they are named and never omitted. A huge const stays in the outline until a later change adds `value` as a fold range.

`function_signature_item` has no body. It is a signature already. It is named
so a focus miss can be reported, and it is never omitted.

A trait or impl declaration list is a container body. At depth 1 those
containers are not eligible to fold, so the query capture is ignored for
omission and the methods inside it are the fold targets. At depth 0 the
container body is eligible, and one marker replaces the whole declaration
list unless a focused symbol sits inside it.

TypeScript and TSX:

```scheme
(function_declaration
  name: (_) @name
  body: (_) @body) @item

(method_definition
  name: (_) @name
  body: (_) @body) @item

(class_declaration
  name: (_) @name
  body: (_) @body) @item

(interface_declaration
  name: (_) @name
  body: (_) @body) @item

(variable_declarator
  name: (identifier) @name
  value: (arrow_function body: (_) @body)) @item
```

A method name is a `property_identifier`, a string, a number, or a private identifier. The capture is `(_)`. The arrow pattern keeps `name: (identifier)` on purpose: a destructuring pattern is not a symbol, and `symbol_of` would not name it either. An arrow whose body is an expression rather than a `statement_block` still matches, because the body capture is `(_)`.

JavaScript uses that query without the `interface_declaration` pattern.
`interface_declaration` is not a node in the JavaScript grammar, and the
query fails to compile if it is left in. TSX uses the TypeScript query as
written. Python:

```scheme
(function_definition
  name: (_) @name
  body: (_) @body) @item

(class_definition
  name: (_) @name
  body: (_) @body) @item
```

Go:

```scheme
(function_declaration
  name: (_) @name
  body: (_) @body) @item

(method_declaration
  name: (_) @name
  body: (_) @body) @item

(type_declaration (type_spec name: (_) @name)) @item
```

A Go function or method with no body is a declaration. The `body:` pattern does not match it, so it stays as the one line it already is. The method name in this grammar is a `field_identifier`; `(_)` still binds it.

Anonymous closures and lambdas are not items. They have no symbol. They
remain inside whatever body contains them: copied when that body is kept,
gone when that body is a marker.

Locating the ranges:

```rust
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

struct Captured<'tree> {
    item: Node<'tree>,
    /// `None` for a signature with no body field.
    body: Option<Node<'tree>>,
}

fn capture<'tree>(
    query: &Query,
    root: Node<'tree>,
    source: &[u8],
) -> Vec<Captured<'tree>> {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source);
    let mut out = Vec::new();
    let item_ix = query.capture_index_for_name("item").expect("item");
    let body_ix = query.capture_index_for_name("body");
    while let Some(m) = matches.next() {
        let Some(item) = m.nodes_for_capture_index(item_ix).next() else {
            continue;
        };
        let body = body_ix.and_then(|ix| m.nodes_for_capture_index(ix).next());
        // `Node` is `Copy`. QueryMatches is a StreamingIterator: the next
        // advance overwrites the match, so the nodes are copied out first.
        out.push(Captured { item, body });
    }
    out
}
```

This is the tree-sitter 0.25 API. `matches` does not implement `Iterator`; a
`for` loop over it does not compile. `capture_index_for_name` is how a
capture is addressed. `source` is `file.as_bytes()`, borrowed from the same
`String` the tool read. `Node::byte_range` returns `Range<usize>` on UTF-8
boundaries. `Node::utf8_text` returns `&str` into that buffer. The outline
uses it for the symbol and the parameter list, not to build the skeleton.

### Which bodies stay

A captured item with a body becomes a span:

```rust
struct Span {
    symbol: String,
    /// Parameters with whitespace collapsed, capped at 80 bytes. Empty if
    /// the grammar node has no `parameters` field.
    params: String,
    kind: &'static str,
    node: Range<usize>,
    body: Range<usize>,
    parent: Option<usize>,
}
```

`parent` is the nearest span whose body range contains this item's node
range. Spans are collected in preorder, so a parent index is always smaller
than the child index.

Eligibility by depth, applied before selection:

| Depth | Eligible to become a marker |
|---|---|
| 0 | Every span that has a body |
| 1 | Functions and methods. Struct, enum, and interface field lists. Not impl, trait, class, or module bodies |
| 2 | Function and method spans that sit inside another function or method. Top-level function bodies stay |

A focused symbol is not eligible, at any depth. Neither is any span whose
body contains a focused span: the container has to stay open or the focused
body would be inside a marker. Neither is a descendant of a focused span:
focus means the whole inner body, including nested functions.

```rust
fn select(spans: &[Span], direct_hit: &[bool], eligible: &[bool]) -> Vec<usize> {
    let mut covered = direct_hit.to_vec();
    for i in 0..spans.len() {
        if let Some(parent) = spans[i].parent {
            if covered[parent] {
                covered[i] = true;
            }
        }
    }
    let mut omitted_ancestor = vec![false; spans.len()];
    let mut folds = Vec::new();
    for i in 0..spans.len() {
        if let Some(parent) = spans[i].parent {
            if omitted_ancestor[parent] {
                omitted_ancestor[i] = true;
                continue;
            }
        }
        if covered[i] || !eligible[i] {
            continue;
        }
        let holds_focus = spans.iter().enumerate().any(|(j, child)| {
            j != i && direct_hit[j] && spans[i].body.contains(&child.node.start)
        });
        if holds_focus {
            continue;
        }
        omitted_ancestor[i] = true;
        folds.push(i);
    }
    folds
}
```

One pass is enough because parents are numbered first. An outer span that is
omitted marks its descendants `omitted_ancestor`, so the renderer never
receives a range inside another range. The folds it does receive are sorted
by `body.start` and do not overlap. That is the property the renderer trusts.

Rendering writes each kept slice once:

```rust
const MARKER_PREFIX: &str = "<<<ROBI_OMITTED";

struct Fold<'a> {
    body: Range<usize>,
    marker: &'a str,
}

fn render(source: &str, folds: &[Fold<'_>]) -> String {
    let mut out = String::with_capacity(source.len() / 4);
    let mut cursor = 0usize;
    for fold in folds {
        debug_assert!(source.is_char_boundary(fold.body.start));
        debug_assert!(source.is_char_boundary(fold.body.end));
        debug_assert!(fold.body.start >= cursor);
        out.push_str(&source[cursor..fold.body.start]);
        let indent = indent_of(source, fold.body.start);
        out.push('\n');
        out.push_str(indent);
        out.push_str(fold.marker);
        cursor = fold.body.end;
    }
    out.push_str(&source[cursor..]);
    out
}

fn indent_of(source: &str, body_start: usize) -> &str {
    let line_start = source[..body_start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &source[line_start..body_start];
    let end = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(line.len());
    &line[..end]
}
```

`indent_of` borrows the source. The marker is the only synthesized text:

```text
<<<ROBI_OMITTED symbol="PaymentService::validate_expiry" lines=9-12 sha256="a1b2c3d4e5f67890">>>
```

`symbol` is JSON-string escaped, so a quote or backslash in a name cannot
break the line. `lines` is the 1-based inclusive line range of the omitted
body, counted in the file, not in the outline. `sha256` is the first 8 bytes
of SHA-256 over the omitted body bytes, hex, 16 characters. The hash is of
the body only, not of the whole file. `file_sha256` on the result is the
full-file hash.

Imports. When `expand_imports` is false, the leading run of import nodes is
measured with the same kinds the chunker uses (`use_declaration`,
`import_statement`, `import_from_statement`, `import_declaration`). The run
stops at the first top-level node that is not one of those. If that run
covers more than 15 lines, it is one extra fold whose range is
`first.start_byte..last.end_byte` and whose symbol is `imports`. It is not a
symbol the model can focus. `expand_imports: true` leaves the run alone. A
`use` that appears later in the file is ordinary source.

### Size gate

The output cap is 32 KB, the same `MAX_READ_BYTES` as `read_file`.

1. Render at the requested depth.
2. If `content` is over 32 KB and `depth_applied` is 2, render again at
   depth 1.
3. If it is still over and `depth_applied` is 1, render again at depth 0,
   still honoring focus.
4. If a single focused body plus the signatures around it is still over
   32 KB, that body is not truncated mid-function. It is emitted as a marker
   whose symbol is the focused name, and `truncated` is true. `hint` says to
   `read_file` at `start_line`. A prefix of a function is a worse edit target
   than a marker.
5. If depth 0 with no focused bodies is still over 32 KB, `content` is cut
   on the last complete marker boundary under the cap, `truncated` is true,
   and `hint` says to pass a narrower `focus_symbols` or to `read_file` a
   line range. The cut never falls inside a marker.

`depth_applied` reports the depth that was actually rendered.

### Matching a focus string

Compare against each span in this order. The first stage that produces a
unique span wins for that string. A stage that produces several spans stops
the search for that string and records `ambiguous`. It does not fall through
to a looser stage.

1. Exact `symbol`.
2. Exact `symbol` plus the collapsed parameter list, the form
   `PaymentService.process_card(card: Card)`. The model only needs this form
   when stage 1 was ambiguous. The `ambiguous` entry shows it.
3. A unique final path segment. `validate_expiry` matches
   `PaymentService::validate_expiry` when no other span ends in that segment.
   Comparison uses the language separator, so a `.` query does not match a
   Rust symbol and a `::` query does not match a TypeScript symbol.

Whitespace at the ends is stripped. A trailing `()` with nothing inside is
stripped before stage 1, so `process_card()` and `process_card` are the same
focus string. A non-empty parameter list is stage 2 and is not stripped.

The parameter list is the `parameters` field text. Runs of whitespace become
one space. The result is capped at 80 bytes on a char boundary. Two
overloads whose parameters agree on those 80 bytes stay ambiguous, and the
entry says so. The model then uses `read_file` on the line ranges in that
entry.

## Hashes

The omitted body is not written to a second store. The file is the value.
The marker's `sha256` and the result's `file_sha256` are checks the next
call can recompute by reading the file again. A 1 MiB parse is the cost of
that call. A copy of the body in `redb` or `sled` would be a second source
of truth that goes stale the moment the user saves the file in their editor,
and it would duplicate bytes the transcript does not need.

`unfold_symbol` is not a tool. Opening a folded body is `read_code` with
that symbol in `focus_symbols`, or `read_file` at the marker's start line.
Both re-read the file. There is no hash key that returns a cached body.

What the hash is for:

- The model can see, on a later `read_code` of the same path, that
  `file_sha256` changed and the previous markers are not a map of the
  current file.
- `edit_file` does not consult the hash. It refuses the marker prefix
  regardless of whether the body is still current.
- Tests hash a known body and assert the 16 hex characters. The algorithm
  is SHA-256, first 8 bytes, lowercase hex. 8 bytes is enough inside one
  file: the identity of a span is the symbol plus the line range, and the
  short hash only distinguishes two bodies. A collision within one file
  appends another byte until the rendered markers differ. Lookup is never
  by hash alone.

Rejected store, recorded so it is not rebuilt later. A `redb` database at
`~/.robi/outline.redb`, one table:

| Key | Value |
|---|---|
| `session_id \|\| 0xff \|\| path \|\| 0xff \|\| file_sha256 \|\| 0xff \|\| symbol` | `start_byte`, `end_byte`, `body_sha256`, and the body bytes |

`sled` would be the same records in a tree. Both keep a body after the file
has changed, both outlive the decision to delete the session unless a second
gc exists, and both add a crate the process does not have. The session
database is the place session-scoped bytes go when a later milestone needs
them. This one does not.

If a call must prove the outline the model saw still matches the disk, it
passes the previous `file_sha256` as nothing: it calls `read_code` again.
The new result either shows the same hashes or it does not. No compare API.

## Edits and edge cases

### Nested symbols

Selection handles nesting without a special case beyond `parent`,
`covered`, and `holds_focus`.

A focused method inside an unfocused impl or class: the impl is not
eligible at depth 1, so it stays open anyway. Sibling methods are eligible,
are not covered, and do not hold a focused descendant, so each sibling body
is a marker. The focused method is covered, so its nested functions stay
in the copied body.

A focused function inside an unfocused function, depth 2, where the outer
function is eligible: the outer span `holds_focus`, so its body is not
replaced. Unfocused nested functions inside it are still eligible and are
marked. The outer function's straight-line statements stay, because they
are not spans. That can be most of the file. The result is still an honest
outline. The model focuses the outer symbol or uses `read_file` when the
glue itself is the edit.

A focused class or impl, at any depth: the container is a direct hit, so
`covered` propagates to every span inside it. No marker appears inside it.
The rest of the file still folds. Focusing a type is the way to ask for
every method body, and it counts as one of the eight focus strings.

Depth 0 and a focused method inside an impl: the impl body would normally
be one marker. `holds_focus` prevents that. The impl stays open, the
focused method is copied, other methods in that impl are markers, and
unfocused top-level items elsewhere in the file collapse whole.

### The same name twice

Stage 1 and stage 3 do not guess. Two `new` methods in one file are
`Foo::new` and `Bar::new` after `symbol_of`, so they are not ambiguous.
Two TypeScript overloads named `process_card` on `PaymentService` share a
symbol. Stage 1 sees two spans and stops. Nothing is unfolded for that
focus string. `ambiguous` lists both candidates as
`PaymentService.process_card(<parameters>)` plus each line range. The model
sends the parameter form, or it calls `read_file` on one of those ranges.

A unique suffix still works when the qualified names differ.
`validate_expiry` matching both `PaymentService::validate_expiry` and
`RefundService::validate_expiry` is ambiguous at stage 3 and does not open
either body.

A focus string that matches a struct and also a method is ambiguous when
both equal the query. The model sends the longer symbol.

### Markers must not reach the file

`edit_file` and `write_file` reject the call before any read-modify-write
when `old`, `new`, or `content` contains the literal `<<<ROBI_`. That covers
an outline marker and a compressed shell marker
([shell-output.md](shell-output.md)).
The check is a byte search, not a parse of the outline. `replace_all` does
not bypass it.

An outline marker produces:

```text
text contains an omitted-body marker. It is not in the file. Call read_code
with focus_symbols set to the symbol in that marker, or read_file at that
marker's start line, and quote those bytes.
```

When that marker is well formed enough to yield a symbol, the error appends
`symbol: <name>, lines: <start>-<end>`. A `<<<ROBI_LOG` marker produces
`text contains a compressed shell marker. It is not file text.` A malformed
marker is still refused; the prefix is enough.

This is the whole fallback. The write does not try to expand the marker
into the current body and apply the edit. Doing that would write a patch
the model did not see onto a range it did not quote.

A signature copied from an outline is real source and may be `old`. The
lines after it that are a marker may not. A focused body copied from an
outline is real source and may be `old`, braces included.

`write_file` uses the same prefix check on `content`. Replacing a file with
a skeleton would persist the markers. `delete_file` does not look at
content.

A file that already contains the sentinel cannot be edited through any
`old` or `new` that includes it, and cannot be overwritten by `write_file`
while the sentinel is in `content`. The sentinel is reserved. It is not a
comment form, so ordinary `/* omitted */` notes in a repository are
unaffected.

## Rejected alternatives

- **Folding inside `read_file` behind the same name.** The model quotes
  `read_file` output into `edit_file` today. A mode on that tool that
  sometimes returns markers will be quoted. A separate tool with `view` on
  every result keeps the quote source stable.
- **Default `compress` from the caller's intent.** The schema cannot know
  whether this call is a search or the edit. The description states the
  rule, and the default is the outline.
- **`compress: false` together with `focus_symbols`.** That pair reads as
  "the whole file, but also these bodies," which is the file-level switch
  this page exists to avoid. It is an argument error.
- **Truncating a focused body to fit 32 KB.** The model edits the prefix
  and repeats the rest from memory. The body stays a marker and `read_file`
  is the way to see a slice of it.
- **Returning an outline when the parse has errors.** The index turns a
  broken file into line windows so search still hits it. An outline that
  skips an error node drops the region the model most needs. The call
  fails and names `read_file`.
- **Storing omitted bodies in `redb` or `sled`.** The file is still there.
  A hash-keyed copy drifts as soon as the editor saves, and session
  deletion would have to learn about a second database. Re-parsing on the
  next `read_code` recomputes the hash.
- **An `unfold_symbol` tool.** It is `read_code` with one focus string.
  A second name gives the model two ways to do one read.
- **Syntactic comments as markers (`/* omitted */`, `# omitted`).** They
  look like source, they are legal in some of these languages, and a
  search for a short comment collides with real notes. The sentinel is
  invalid syntax on purpose.
- **Omitting a container that holds a focused child.** The focused body
  would sit inside the marker and the model would have no text to quote.
- **Guessing among overloads.** A wrong `new` opened in full is a confident
  edit to the wrong function. Ambiguity returns the candidates and unfolds
  none of them.
- **Sharing this path with M9.** M9 passes source through because a crushed
  file is how the model edits the wrong lines. An outline is safe only
  because the model asked for it and because writes reject the marker.
  Applying it inside the compressor would fold results the model expected
  to quote, including `read_file`.

## Failure modes

- A denied path, a missing file, a directory, a non-UTF-8 file, and an
  unset `$HOME` on a `~/` path use the `read_file` errors. Nothing is parsed.
- A binary file (NUL in the first 8 KB) is `file is binary`.
- A file over 1 MiB is `file is too large; use read_file with offset and limit`.
- No grammar: a source window, `view: "source"`, `reason: "no grammar"`.
  Not an error.
- `root.has_error()`: `file did not parse; use read_file`. No partial outline.
- `focus_symbols` longer than 8, a focus string outside 1..=200 bytes, or
  `focus_symbols` / `depth` / `expand_imports` / `offset` / `limit` combined
  with the wrong `compress` value: `invalid arguments`. The file is not read.
- A focus string with the wrong separator for the language matches nothing
  and is listed in `missing`. The call still returns the outline.
- Two spans for one focus string: neither body is copied. `ambiguous` carries
  the parameter forms and line ranges.
- Output over 32 KB: depth steps down, then a focused body becomes a marker
  with `truncated: true`. The result does not contain a cut function.
- `edit_file` or `write_file` text containing `<<<ROBI_`: the write
  does not start. The file is unchanged.
- The file changes between the outline and the edit. Exact match fails with
  `old text was not found`, as it does for any other stale quote. The marker
  hash is not consulted and is not patched in.
- Cancellation during the read returns `cancelled` and no result. Parse and
  render hold no lock beyond the read.
