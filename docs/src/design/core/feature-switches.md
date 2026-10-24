# Feature switches

The streamlining roadmap removed backend surfaces without deleting them. The
front end still has one switch for that, `src/app/surfaces.ts`. This page
describes its backend counterpart: one Cargo feature per workstream, in
`crates/robi/Cargo.toml`, all off by default.

Read [streamlining.md](../../streamlining.md) for what each workstream removes.

## The flags

| Feature | Restores | Workstream |
|---|---|---|
| `read-tools` | `read_file`, `list_dir`, `find`, `grep` | S1 |
| `review` | the `/chat_sessions/{id}/review` routes and the baseline reader | S2 |
| `edit-tools` | `write_file`, `edit_file`, `delete_file` | S1 |
| `docs-viewer` | `/workspaces/{id}/docs` and its literal search | S5 |

`default = []`. The streamlined harness ships; a feature is the return path.

S3 (the semantic index) and S4 (the language-server tools) are past the feature
stage. Their code, `crates/robi-index`, and the LSP client are deleted, so no
flag restores them. See [streamlining.md](../../streamlining.md) for what a
return would need.

## The edges

One feature edge is left, and it is not decoration:

- **`edit-tools = ["review"]`** — the edit tools and their tests use
  `review::{diff, hunks_for_session, reject}` to render a change. Writes without
  the review half would be writes with no diff.

Two dependencies arrive with a feature rather than with the default build:

- **`read-tools = ["dep:globset", "dep:ignore"]`** — `find` matches with
  `globset`, and `find` and `grep` walk with `ignore`.
- **`docs-viewer = ["dep:ignore"]`** — the docs listing and its literal search
  walk with `ignore`.

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
