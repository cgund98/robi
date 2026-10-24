# S1 task set

The streamlining bet is empirical: a minimalist harness with a good prompt
outperforms a complex one. This page is the run that decides it. It is the exit
criterion for S1 in [streamlining.md](../streamlining.md#s1--shell-first-editing),
and it runs against two builds of the same commit:

```sh
# The before: the tools the roadmap kept.
cargo run -p robi --bin robi-api --features read-tools,edit-tools

# The after: shell only, which is the default build.
cargo run -p robi --bin robi-api
```

Everything else is held equal: the same model, the same model settings, the same
`AGENTS.md`, the same workspace at the same commit, and the same user
instruction. Do not tune the prompt between runs. The point is to compare the
harnesses, not to make the new one win.

## The tasks

Each is one session, started from a clean tree, in this repository. Together they
cover the four jobs the tools used to own: read, search, edit, verify.

| # | Task | What it is testing |
|---|---|---|
| T1 | "Read `crates/robi/src/agent/tools/shell.rs` and tell me what `Shell::execute` does when the command fails" | A read of a known file. `read_file` versus `sed`/`cat`. |
| T2 | "Where is `ShellAccess` decided for each mode, and what else would need to change to add a mode?" | A search across files. `grep`/`find` versus `grep -rn`. |
| T3 | "In `crates/robi/src/agent/tools/shell.rs`, add a `hint` to the payload when the command exits non-zero but the sandbox did not block anything. Do not change the blocked-path hint." | A small exact edit in a known file, plus verification. |
| T4 | "Add a `--verbose` note to `docs/src/guides/editor.md`'s 'See what changed' section, then confirm the markdown still builds" | An edit plus a real verification command. |
| T5 | "Why does a read-only shell refuse `cargo check`, and what would it take to allow it?" | Reasoning over the sandbox profile, with no edit. |
| T6 | "Rename the `WriteAccess::ReadWrite` variant to `ReadWrite` … " — pick a task from your own backlog | An unfamiliar edit, so the result is not memorised. |

T6 exists because the first five are in a repository the model has seen. Choose a
one-file change in a workspace the model has not been trained on, keep it
identical across both runs, and discard the task if either run touches a second
file.

## The metrics

Record one row per task per build. Six numbers, because one is not enough:

| Metric | Why it matters | Where it comes from |
|---|---|---|
| Task success | The only one that matters if it moves the wrong way | Did the edit land, and does it do what was asked? Read the diff. |
| Model turns to done | A shell-first agent that needs ten extra turns is not simpler | The loop records an iteration per turn |
| Tool-call errors | Shell syntax mistakes, bad paths, failed replacements | `tool failed` and `invalid arguments` in the log |
| Tokens in and out | The prompt gets longer and the schemas get shorter; measure the net | Provider usage on the turn |
| Time to first useful diff | Perceived speed, not wall-clock | First `file_changed` or `git diff` in the trace |
| Approval prompts per task | Fewer prompts is the point; more means the sandbox is doing too much | `awaiting_approval` frames |

The roadmap's exit criterion is stated against the first three: **the same
number of turns as today, with no higher rate of failed commands.** If the
shell-only build does not clear that, the answer is not to rewrite the prompt
first — it is `--features read-tools`, which restores the read half alone and
leaves editing on the shell.

## Reading the traces

A single number will not decide this. Read the turns where the shell-only build
lost, and classify each loss:

| Loss | Fix |
|---|---|
| The model does not know a command form | A prompt hint in `agent/prompt/editing.rs` |
| The model knows the form and needs three calls to do what one tool did | A missing tool; consider `read-tools` |
| The command is refused by the sandbox | A policy question, not a prompt question |
| The model gives up and asks the user | A prompt ordering problem, usually |

That distinction — prompt hint versus missing tool — is the roadmap's main open
question, and it is a judgement call over traces, not a score.

## What was already measured without running this

Two things are facts rather than measurements, and they are in the plan:

- The default build drops `tree-sitter`, `fastembed`, and `sqlite-vec` from the
  dependency graph. `cargo tree -p robi | grep -E 'tree-sitter|fastembed|sqlite-vec'`
  prints nothing. A later change deleted the crates outright, so no feature
  brings them back.
- A cold workspace start issues no index work, because there is no `IndexHub`
  and no index task left in the tree.

The task set above is what still needs a human at the keyboard.
