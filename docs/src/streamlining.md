# Streamlining Roadmap

This page is a proposal, not a plan of record. It sketches a smaller Robi: one
general-purpose tool instead of a suite of specialised ones, fewer surfaces, and
a shell that spends its width on the conversation. Nothing here is built yet.

The premise is that a **minimalist harness** can outperform a complex one. A
small tool set with good prompts gives the model fewer ways to choose wrong, and
a shell is the one interface that already knows how to read, search, edit, and
verify a repository. The counter-premise is just as important: every tool we
remove is a guarantee we stop enforcing in code and start enforcing in prompt
copy. This roadmap exists to test the first claim without losing the second.

Read [roadmap.md](roadmap.md) first for the shipped milestones. This page
describes a possible M11 that subtracts from them.

## Thesis

| Complex harness | Minimal harness |
|---|---|
| A tool per operation (`read_file`, `edit_file`, `write_file`, …) | One `shell` tool, plus a small set of read-only search tools |
| Tool schemas encode safety | The sandbox and approval policy encode safety |
| In-app review, docs, and index panes | The IDE, git, and a file browser do those jobs |
| A permanent navigation column | A top bar that yields width to the transcript |

The bet is that a strong model with `grep`, `cat`, `git diff`, and a shell writes
better code than a weaker model constrained by bespoke tools. If the bet is
wrong, the model burns turns on shell syntax and the harness gets slower, not
simpler. The measurement plan below is how we tell the difference.

## What changes

| # | Workstream | Outcome |
|---|---|---|
| S1 | Shell-first editing | `read_file`, `edit_file`, and `write_file` are disabled. The agent reads and edits through `shell`, with prompt hints and `git diff` verification |
| S2 | No in-app code review | Review routes, screens, and baselines leave the app. Review happens in an IDE or `git diff` |
| S3 | No semantic index | Indexing, embeddings, the `semantic_search` tool, and index UI are disabled |
| S4 | No LSP diagnostics | The `diagnostics` tool is disabled, and the LSP surface is reduced or gated |
| S5 | No docs viewer | The `/docs` route, docs screen, and docs editor are temporarily removed |
| S6 | Top-bar shell | The permanent left sidebar is removed; navigation moves into the top bar |
| S7 | Read-only shell in Ask and Plan | Ask and Plan get a shell that cannot write, whatever the sandbox allows |
| S8 | Read-only subagents | No child can write, and none can run unsandboxed |

### Disable, do not delete

Every workstream follows the same rule: **remove the surface, keep the code**.
Concretely, a disabled feature:

- is not registered in the tool registry or the router, so the model and the
  client cannot reach it;
- is not described in the system prompt, the tool catalog, or `reference/tools.md`;
- has no entry point in the UI;
- stays in a module behind one switch, so restoring it is a config change rather
  than a revert.

The switch should be one place, not six. A `streamlined` setting (or a
`legacy-harness` Cargo feature) read once at composition time is enough. A
feature with no switch and a commented-out `register_*` call is not a disabled
feature; it is a merge conflict waiting to happen.

Design pages for disabled features stay in the book with a status banner at the
top: **Disabled in M11. Kept for reference.** A page that silently describes a
screen the app no longer has is worse than no page.

## S1 — Shell-first editing

Disable the three file-mutation tools and let `shell` do the work. `find`,
`grep`, and `list_dir` stay: they are read-only, cheap, and map cleanly to what
the model already knows. `read_code` stays for the same reason, or is disabled
too if it goes unused; decide from the tool-call data, not from taste.

### What the model is told

The prompt is the whole interface now, so the hints are product copy and need
the same review as a tool schema. The first cut:

| Job | Hint |
|---|---|
| Read a file | `sed -n '1,120p' path`, `cat path`, `head`/`tail`; no pager, no `less`, no `vim` |
| Read a slice by line | `sed -n '40,80p'` or `awk 'NR>=40 && NR<=80'` |
| Full overwrite | `cat > path <<'EOF' … EOF` — quoted delimiter, so `$` and backticks survive |
| Small exact replacement | A short Python script that reads, replaces one occurrence, asserts the count, and writes |
| Delete | `rm path`, after confirming the path with `ls` |
| Verify | `git diff -- path`, `git status --short`; read the diff before claiming success |
| Avoid | `sed -i` for multi-line or regex-heavy edits, interactive editors, commands that wait on stdin |
| Search | `grep`/`rg`; prefer the tools when they are registered |

Two hints carry most of the risk:

- **`sed -i` is the wrong default.** It is portable enough to seem safe and
  subtly different between GNU and BSD, and an unanchored pattern edits more
  than intended. A Python replace that asserts exactly one match fails closed.
- **`cat <<'EOF'` is the only reliable full overwrite.** An unquoted delimiter
  expands `$VAR` and backticks inside the file being written, which corrupts
  shell scripts and templates. The prompt must show the quoted form.

The existing `shell` tool already returns `exit_code`, `stdout`, `stderr`,
`truncated`, and `sandboxed`, plus a blocked-path hint. That is the feedback
loop the agent needs. Shell output compression (M9) becomes more important here,
because a shell-first agent produces more output, not less.

### What this costs

- **Checkpoints can no longer be recorded automatically.** The edit journal in
  [checkpoints.md](design/tools/checkpoints.md) relies on `write_file` and
  `edit_file` to capture a baseline before their first write. A `shell` command
  is opaque, so the harness cannot know which paths it touched. S1 therefore
  depends on S2: either review and undo move to git, or a shell-first session
  has no per-session undo. Pretending to keep checkpoints without intercepting
  writes would be worse than removing them.
- **Ask and Plan must be pinned read-only first.** Once the shell is the editing
  interface, a mode that has a shell has an editor. S7 lands before S1 for that
  reason; without it, the mode policy silently widens.
- **Approval granularity changes.** A denied path currently becomes a prompt for
  that call. Under a shell, approval is for the command, and the command can
  touch many paths. The sandbox and the session path rules must stay the
  enforcement point; the prompt hints are convenience, not a boundary.
- **The prompt gets longer.** Deleting three schemas buys some tokens back, but
  the shell hints spend them. Measure the net change rather than assuming it is
  a saving.

### Exit criteria

A representative edit task — read a file, change one function, run the tests,
and inspect `git diff` — completes in the same number of turns as today, with no
higher rate of failed commands. If it does not, keep the edit tools.

## S2 — No in-app code review

Remove the review screen and the routes behind it:

- `GET /api/v1/chat_sessions/{id}/review`
- `GET /api/v1/chat_sessions/{id}/review/file`
- `POST /api/v1/chat_sessions/{id}/review`
- the `/sessions/:id/review` route and `src/features/review/`

The user reviews in an IDE or with `git diff`, which is where review already
happens for most people. The assistant can be asked to explain a diff, but it
does not own a review surface.

This is coupled to S1 and to [checkpoints.md](design/tools/checkpoints.md):

- The review screen reads `session_file_baselines`, and so does checkpoint
  undo. Removing review does not automatically remove the table, but a
  shell-first agent cannot fill it.
- Recommendation: **remove the baselines with the review screen.** Git becomes
  the undo mechanism. A session that wants an independent snapshot can ask for
  `git stash` or a branch, under the same approval as any other command.
- Keep `crates/robi/src/agent/review/` behind the disabled switch until the
  shell-first path has shipped for a release. If checkpoints return, they will
  return as a git-backed feature, not a second journal.

Also update [guides/code-review.md](guides/code-review.md): either delete the
page or turn it into "reviewing changes with git and your editor". A guide for a
removed screen is a broken link with extra steps.

### Exit criteria

No route, component, guide, or prompt mentions in-app review. `git diff` is the
documented verification path, and the S1 prompt hints say so.

## S3 — No semantic index

Agents do not call `semantic_search` enough to justify an embedding runtime,
a vector store, and a background index task. Disable:

- the `semantic_search` tool (`crates/robi/src/agent/tools/semantic_search.rs`);
- `IndexHub` and its leases (`crates/robi/src/agent/index.rs`);
- `GET`/`PUT /api/v1/workspaces/{id}/index` (`web_api/code_index.rs`);
- `IndexStatusLine` in the chat header;
- the semantic engine in docs search, assuming docs search survives S5 at all.

`crates/robi-index` stays in the tree, unbuilt or feature-gated. It is the
largest optional dependency in the workspace, which is exactly why it should be
behind a feature rather than deleted: restoring it later is a manifest change.

`grep` and `find` become the only retrieval path. That is a real downgrade for
"where is this concept handled?" questions, and the prompt should say so
honestly: search by identifier and filename first, then read the hits.

### Exit criteria

A cold workspace start does no indexing work, opens no embedding model, and the
index status UI is gone. `grep`/`find` cover the retrieval in the S1 task set
without a measurable drop in success.

## S4 — No LSP diagnostics

Disable the `diagnostics` tool, the settings toggle, and the registration path
in `register_read_tools`. The two smaller decisions:

1. **Diagnostics only, or the whole LSP surface?** `definition`, `references`,
   `hover`, and `workspace_symbol` share the same hub. If the agents do not use
   diagnostics, they are unlikely to use symbol navigation either. Prefer
   disabling the whole set and keeping `crates/robi/src/agent/lsp/` behind the
   switch. If tool-call data says `grep` is not a substitute for `references`,
   restore the four non-diagnostics tools and leave diagnostics off.
2. **What replaces verification?** The shell. `cargo check`, `tsc`, `eslint`,
   and the project's own test command are already available through `shell` and
   are more honest than a diagnostics snapshot. Add them to the prompt hints.

`docs/src/reference/tools.md` should lose the language-server section, and
[design/intelligence/lsp.md](design/intelligence/lsp.md) gets the disabled
banner from S0.

### Exit criteria

`lsp = on` no longer registers a tool, the settings toggle is gone, and the S1
task set verifies with `cargo check` rather than `diagnostics`.

## S5 — No docs viewer

Temporarily remove the viewer and editor:

- the `/docs` route and `ViewToggle`;
- `src/features/docs/` (screen, tree, toolbar, find bar, editor, autosave);
- `src/api/docs.ts` and the docs store.

The backend question is separate. The agent can read and write markdown with the
shell, so `web_api/docs.rs` is not needed by the model. Recommendation: disable
the docs routes with the UI, and keep `agent/docs/` (change reconciliation and
merge) behind the switch. When the viewer returns, it returns with its merge
path intact rather than half-remembered.

Docs search ([guides/documentation-search.md](guides/documentation-search.md))
goes with the viewer. If search should survive as a standalone surface, it needs
its own entry point, which is a different feature.

**Temporarily** means the removal is reversible and recorded: the design pages
[design/shell/docs-viewer.md](design/shell/docs-viewer.md) and
[design/shell/docs-mode.md](design/shell/docs-mode.md) stay, with the banner and
a note on what a return would need.

### Exit criteria

No `/docs` route, no docs components, and the S1 prompt tells the agent to use
the shell for markdown. The user browses docs in an external editor.

## S6 — Top-bar shell

Remove the permanent left sidebar and give the transcript the full window width.
Everything the sidebar held moves into the top bar:

| Today (sidebar) | Tomorrow (top bar) |
|---|---|
| Workspace switcher | Workspace dropdown at the leading edge |
| Recents session list | Recents dropdown, with the current session title as the trigger |
| New chat | Icon button |
| Workspaces | Item in the workspace dropdown, not a separate page link |
| Settings | Icon button at the trailing edge |
| Chat/Docs toggle | Gone with S5 |
| History back/forward | Icon pair beside the title |
| Index status | Gone with S3 |
| API status, MCP tray | Stay in the top bar; MCP moves into an overflow icon |

The shell already has two vertical layers to work with: `WindowFrame` (the
draggable, route-tinted window chrome) and `ChatHeader` (the session header).
S6 should produce one bar, not two stacked bars. On macOS the leading edge
must leave room for the traffic lights; the existing `WindowFrame` padding rules
are the place to handle that.

Design constraints:

- **One row at the default width.** The title truncates before any control does.
  The primary actions (workspace, recents/new chat, settings) stay visible; the
  rest collapses into one overflow menu.
- **Dropdowns are menus, not panels.** Kobalte primitives styled with the tokens
  in [visual-style.md](design/shell/visual-style.md). No new hex values.
- **Keyboard and focus still work.** Every icon-only button needs an accessible
  name and a shortcut where the sidebar had one. A hover-only menu is not a
  replacement for a visible list.
- **Recents needs search.** A dropdown is worse than a list once a user has more
  than a screenful of sessions. Ship it with a filter field or cap it and link
  to the workspaces page.
- **State lives where it did.** `ChatChrome` keeps owning route and session
  state; only the presentation moves. Do not create a second source of truth for
  the selected workspace.

### Exit criteria

At 1280 px and 900 px wide, the transcript gets the full width, every sidebar
function is reachable in two clicks or fewer, and no control is clipped. The
`Sidebar.tsx` and `Sidebar.module.css` files are gone from the render path,
along with `AppLayout`'s fixed left column.

## S7 — Read-only shell in Ask and Plan

Today the mode's tool set decides who may write, and it does so by withholding
tools: Ask gets no `shell`, Plan gets a `shell` that can write anywhere the
sandbox allows, and Agent gets the edit tools too. Under S1 the shell becomes
the editing interface, so "Plan has a shell" quietly becomes "Plan can edit".
That must not happen.

Make read-only a property of the `shell` tool instance, not a consequence of
which tools were registered:

| Mode | Today | After S7 |
|---|---|---|
| Ask | No `shell` | `shell`, read-only |
| Plan | `shell`, full write inside the sandbox | `shell`, read-only |
| Agent | `shell`, full write inside the sandbox | unchanged |

Implementation shape:

- **A flag, not a second tool.** `Shell::new(ctx)` gains a `ShellAccess::ReadOnly`
  or `ReadWrite`. One tool, one schema, two policies; a separate `read_shell`
  tool would split the prompt and the tests for no benefit.
- **Enforce in the sandbox profile, not in the arguments.** A read-only call
  puts the workspace in `Profile::deny_writes` and refuses `write_paths`
  outright. Checking the command string for `>`, `rm`, or `tee` is not
  enforcement: `python -c`, `cp`, and `git checkout` all write without a
  redirect. The OS profile is the boundary.
- **Drop the write knobs from the schema.** In read-only mode, omit `write_paths`
  and `unsandboxed` from `parameters()`, and say so in `description()`. A
  property the model can set and the tool then refuses is a wasted turn.
- **Refuse, then explain.** A blocked write returns the sandbox's blocked-path
  hint plus "this mode is read-only", so the model asks the user to switch to
  Agent rather than retrying the same command.
- **`git` is read-only or it is not.** `git status`, `diff`, `log`, `show`, and
  `grep` are fine. `git checkout`, `restore`, `stash`, `commit`, and anything
  that writes `.git` are writes. The simplest rule that holds: the read-only
  profile denies `.git` writes along with the working tree, which makes the
  whole family fail closed without a command allowlist.

**Build caches are the sharp edge.** `cargo check` writes to `target/`, `tsc`
and `eslint` write caches, and a test run writes artifacts. A blanket
deny-writes profile therefore refuses exactly the verification commands S1
tells the agent to use. Two honest options:

1. **Scope the exception.** Let a read-only call write to the known build and
   cache directories (`target/`, `node_modules/.cache/`, `.git` excluded), and
   nothing else. This needs a per-project list, so it is a setting, not a
   constant.
2. **Verify in Agent only.** Ask and Plan read and reason; they do not run the
   build. The prompt says so, and a refused `cargo check` is expected.

Prefer the first if the mode prompts promise verification, and the second if
they do not. Do not silently allow the whole workspace and call it read-only.

Ask gains a shell it did not have. That is deliberate: with S1 and S3 in place,
`grep`, `ls`, and `git diff` are how the agent answers questions, and Ask needs
them. If Ask should stay tool-free of the shell, drop that row; do not keep the
shell and leave it unconstrained.

### Exit criteria

In Ask and Plan, a command that writes fails inside the sandbox with a
read-only refusal and no file changes. `git diff` still works. The tool schema
in those modes has no `write_paths` or `unsandboxed`.

## S8 — Read-only subagents

A subagent cannot ask for approval, and the current code already fails closed:
`FailClosed` turns any `NeedsApproval` call into an `access_denied` result. That
is the right shape, but it is not the same as read-only. A sandboxed write
inside the workspace needs no approval, so a `general` child can still edit
files, run `git checkout`, and leave the workspace changed after the summary is
returned.

Make the child's access read-only in the profile, not in the wrapper:

- **Every child gets `ShellAccess::ReadOnly`.** `general` keeps its shell,
  because command output is what it is for; it loses the ability to write.
  `explore` has no shell and is unaffected.
- **No child can run unsandboxed.** This is already true through `FailClosed`,
  but state it as an invariant and test it directly rather than relying on the
  wrapper. The child registry must not see `unsandboxed`, `write_paths`, or
  `network: unrestricted` in its schema at all. Removing the key is stronger
  than rejecting the value.
- **Keep `FailClosed`.** It is still what stops a child from reading outside the
  session's allowed paths. Read-only narrows writes; fail-closed still bounds
  reads.
- **The summary is the only channel out.** A child that wants a change makes
  it the parent's job: the summary says what to change, and the parent edits
  with its own approval path. That is the same argument as
  [subagents.md](design/core/subagents.md) makes for not giving `explore` a
  shell.

Two consequences to write down:

- `general` subagents become slower for tasks that assume they can fix what they
  find, because every fix becomes a parent turn. That is the price of not having
  an approval channel, and it is better than a silent edit.
- A subagent's transcript is in memory and the parent sees only the summary, so
  a write made by a child is the hardest kind of change to audit. Removing that
  path also removes a class of "where did this edit come from?" bugs.

### Exit criteria

No child tool schema contains `unsandboxed`, `write_paths`, or an unrestricted
network option. A `general` child asked to write a file returns a refusal and
the file is unchanged. A test asserts the read-only profile for both child
modes, and a second test asserts the schema has no escalation keys.

## Sequencing

The order is chosen so the app is usable at every step and each step can be
reverted alone.

1. **S0 — the switch and the banners.** Add the single `streamlined` switch, the
   disabled banners, and a prompt-hint test that asserts the shell hints are
   present. No behavior changes.
2. **S6 — the top bar.** Pure UI. Do it before removing features so the layout
   is proven while the surfaces still exist. The sidebar can be hidden first,
   then deleted.
3. **S5 — the docs viewer.** Smallest backend surface, and its removal frees the
   top bar of the Chat/Docs toggle.
4. **S3, S4, and S8 — the intelligence layer and child confinement.** All three
   are backend registrations plus a policy flag. None changes the editing path,
   and S8 is a small, self-contained narrowing.
5. **S2 — review and baselines.** Remove after the intelligence layer, because
   it is the only workstream that deletes a safety net.
6. **S7 — the read-only shell.** Land it before S1, so the mode policy exists
   before the shell becomes the editing interface. S7 on its own is a small
   behavior fix; it is required only because of S1.
7. **S1 — shell-first editing.** Last, because it is the change that can
   actually make the product worse. It ships behind the switch and is measured
   against the current harness.

S1 depends on S2's decision about baselines and on S7 being in place. If S1
ships without S7, Plan and Ask become editing modes by accident. S3, S4, and S8
must ship before S1's measurement run, so the "before" and "after" harnesses
differ only in the editing path. If S1 fails its exit criteria, S2–S8 still
stand on their own.

```mermaid
graph LR
  S0[S0 Switch] --> S6[S6 Top bar]
  S0 --> S5[S5 Docs viewer]
  S0 --> S3[S3 Semantic index]
  S0 --> S4[S4 LSP]
  S0 --> S8[S8 Read-only subagents]
  S5 --> S2[S2 Review]
  S3 --> S2
  S8 --> S1[S1 Shell-first editing]
  S2 --> S7[S7 Read-only shell]
  S7 --> S1
  S4 --> S1
```

## How we will know

The research claim is empirical, so the change is too. Run the same task set
against the current harness and the streamlined one, with the same model and
the same prompt:

| Metric | Why |
|---|---|
| Task success | The only metric that matters if it moves the wrong way |
| Model turns to done | A shell-first agent that needs ten extra turns is not simpler |
| Tool-call errors | Shell syntax mistakes, bad paths, failed replacements |
| Tokens in and out | The prompt gets longer and the schemas get shorter; measure the net |
| Time to first useful diff | Perceived speed, not wall-clock |
| Approval prompts per task | Fewer prompts is the point; more means the sandbox is doing too much |

Keep the task set in the repository so the comparison is repeatable. A
qualitative review of the traces is worth more than a single number: read the
turns where the streamlined harness lost, and decide whether the loss is a
prompt hint or a missing tool. That distinction is this roadmap's main open
question.

## Risks

| Risk | Mitigation |
|---|---|
| Shell-first editing is less safe, not just simpler | The sandbox and approval policy are unchanged. `shell` stays mandatory-sandboxed; path rules stay the floor. Prompt hints are never described as a boundary. |
| The model writes a broken heredoc and corrupts a file | Quoted delimiter in every example; verify with `git diff` before claiming success; keep `write_file` one switch away. |
| No per-session undo | Git is the undo. Say so in the guide and the prompt. |
| Removing review makes the agent's changes invisible | The chat already streams tool calls; the prompt requires a `git diff` after edits. |
| The top bar cannot hold the navigation at small widths | Overflow menu for secondary actions, keep the primary three visible, test at 900 px. |
| "Disabled" code rots | One switch, one banner convention, and a checklist in this page. Restore is a tested path, not a hope. |
| We remove a tool the model actually used | Keep a month of tool-call counts before deleting the registration. If a tool is in the top five, it does not qualify. |
| A read-only shell blocks `cargo check` and test runs | Scope writes to build and cache directories, or move verification to Agent mode and say so in the prompt. |
| A read-only shell is enforced by scanning the command string | Do not scan. Use the OS profile's deny list. A regex on `>` is defeated by `python -c` on the first try. |
| Plan mode becomes useless without a writable shell | `write_plan` is a separate tool and stays. If a plan needs a scratch file, that is an Agent task. |
| Subagents lose the ability to fix what they find | Expected. The summary names the fix; the parent applies it with its own approval. Measure the extra parent turns in the S1 run. |
| S8 is "already true" and silently regresses | Assert the invariant with tests on the profile and the schema, not on the wrapper's behavior. |

## Open decisions

- **D1 — Is `read_file` really redundant?** `cat` and `sed` are fine, but
  `read_file` reports truncation with an exact continuation offset, and that is
  hard to reproduce in a one-liner. Keep it if the traces show repeated
  `sed`/`wc` calls.
- **D2 — Do checkpoints return as git-backed?** S1 needs an answer before it
  ships. The likely shape: no automatic journal, a prompt hint to use a branch
  or `git stash`, and no in-app undo.
- **D3 — Diagnostics only, or all LSP tools?** See S4.
- **D4 — Do docs routes leave with the UI, or stay for a future client?** The
  recommendation is to disable them with the UI.
- **D5 — One switch or two?** `streamlined` for S1–S5, and a separate flag for
  S6, so the layout can ship without the harness changes. The UI flag should
  not gate backend behavior.
- **D6 — What happens to `read_code`?** It is a read tool, not an edit tool, and
  it depends on the same tree-sitter work as the index. Decide from usage.
- **D7 — Does Ask get a shell?** S7 gives it a read-only one. Ask currently has
  none. Either is defensible; an unconstrained shell in Ask is not.
- **D8 — Build caches under a read-only profile?** Scope the write exception to
  `target/` and friends, or move verification to Agent. See S7. This decides
  whether Ask and Plan can run their own checks.
- **D9 — Do `general` subagents keep a shell at all?** S8 keeps it read-only.
  If the read-only shell turns out to be useless for them, remove the shell from
  both child modes and make delegation purely read-and-summarize.

## What must be updated when this ships

- [roadmap.md](roadmap.md): M6, M7, and M10 gain a "Disabled" note, and M11
  points here.
- [reference/tools.md](reference/tools.md): the tool set after S1, S3, and S4.
- [guides/editor.md](guides/editor.md): the shell-first workflow.
- [guides/code-review.md](guides/code-review.md): review with git and an IDE.
- [guides/documentation-search.md](guides/documentation-search.md): removed with
  S5.
- [design/shell/frontend-layout.md](design/shell/frontend-layout.md) and
  [design/shell/visual-style.md](design/shell/visual-style.md): the top-bar
  layout and the removal of the left column.
- [design/core/agent-modes.md](design/core/agent-modes.md): the mode table, with
  a read-only shell in Ask and Plan.
- [design/core/subagents.md](design/core/subagents.md): `general` children are
  read-only, and no child can escalate.
- [design/tools/shell-tool.md](design/tools/shell-tool.md): the `ShellAccess`
  flag and the read-only profile.
- The design pages for review, LSP, semantic search, and docs: the disabled
  banner from S0.
- `AGENTS.md`: the layout table and the "read/edit tools" language, once the
  change is no longer a proposal.
