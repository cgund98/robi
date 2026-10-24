# Feature switches

The streamlining roadmap removes backend surfaces without deleting them. The
front end has one switch for that, `src/app/surfaces.ts`. This page describes
its backend counterpart: one Cargo feature per workstream, in
`crates/robi/Cargo.toml`, all off by default.

Read [streamlining.md](../../streamlining.md) for what each workstream removes.

## The flags

| Feature | Restores | Workstream |
|---|---|---|
| `semantic-index` | the `semantic_search` tool, `IndexHub`, the `/workspaces/{id}/index` routes, embeddings | S3 |
| `read-tools` | `read_file`, `read_code`, `list_dir`, `find`, `grep` | S1 |
| `lsp-tools` | `diagnostics`, `definition`, `references`, `hover`, `workspace_symbol` | S4 |
| `review` | the `/chat_sessions/{id}/review` routes and the baseline reader | S2 |
| `edit-tools` | `write_file`, `edit_file`, `delete_file` | S1 |
| `docs-viewer` | `/workspaces/{id}/docs` and its search | S5 |

`default = []`. The streamlined harness ships; a feature is the return path.

## The edges

Three feature edges are not decoration. Without them the `--all-features` build
does not compile, and the default build would not tell you:

- **`docs-viewer = ["semantic-index"]`** — `web_api/docs.rs` imports
  `code_index::IndexStatusBody` and calls `state.index.acquire`, `.status`,
  `.index` and `embed_query` inside `search_docs`. A docs viewer without an index
  has nothing to search.
- **`edit-tools = ["review"]`** — the edit tools and their tests use
  `review::{diff, hunks_for_session, reject}` to render a change. Writes without
  the review half would be writes with no diff.
- **`read-tools` and `semantic-index` both need `dep:robi-index`** — `read_code`
  uses tree-sitter `outline`; `semantic_search` and `IndexHub` use the store and
  the embedder. Only `semantic-index` needs `local-embed`, so `read-tools` brings
  tree-sitter back without `fastembed`.

`review/diff.rs` is **unconditional**: `agent/docs/merge.rs` is kept in the tree
(S5) and needs it. Only `review/session.rs` is gated.

## The rule

Gate three things together, and gate nothing else:

1. the **module** (`#[cfg(feature = "…")] mod …;`),
2. its **registration** in `tools/mod.rs` or `web_api/mod.rs`,
3. its **OpenAPI entry** — every `paths(...)` and `schemas(...)` it contributes.

Never gate a handler body. Never gate an item in `robi-core`: the loop learns
nothing about these features. A struct field only a gated module reads goes with
that module, with `#[cfg]` on the field line itself, in the struct and in every
literal that sets it.

## Testing the restore path

The default build compiles no gated module, so a wrong `#[cfg]` is invisible
until every feature is on:

```sh
make lint-restore   # cargo clippy --workspace --all-targets --all-features
make test-restore   # cargo test --workspace --all-features
```

CI runs both in the `rust-restore` job. Treat a change to the feature graph as a
change to that job's cost, not to its existence.

`--all-features` is not the old behaviour. With every feature on, Ask has a
read-only shell it never had and subagents cannot write — that is S7 and S8, and
it stays on in both builds.
