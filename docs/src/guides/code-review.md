# Reviewing changes with git

Robi edits your workspace directly. There is no review screen inside the app:
once a tool call runs, the change is on disk, and the record of it is your git
repository plus the tool calls in the transcript.

This is the same loop you would use for a teammate's branch. Read the diff, keep
what you want, throw away what you do not.

## See what a session changed

```sh
git status --short        # paths this session touched
git diff                  # everything uncommitted, in full
git diff -- path/to/file  # one file
```

The chat helps here: every edit and shell call streams into the transcript as it
runs, so you can read which paths a turn touched without leaving the app. Ask
the assistant to explain a diff and it reads the same output.

If the workspace is not a git repository, `git init` first, or ask the assistant
to. Without version control there is no undo, so start one before a large task.

## Keep or discard

| Goal | Command |
|---|---|
| Throw away one file's changes | `git checkout -- path/to/file` |
| Throw away everything uncommitted | `git reset --hard` |
| Save the current state before a risky turn | `git stash` (restore with `git stash pop`) |
| Keep a change | `git add -p` then `git commit` |
| Work on a throwaway branch | `git switch -c attempt-1` |

On a new file, `git checkout --` does nothing until it is tracked; delete it
with `rm` or `git clean -i` instead.

## Approvals are not review

Approving a tool call is a decision about **running** the command, not about
keeping its result. Approving `write_file` writes the file. The change stays
until git says otherwise.

That distinction is why the diff matters. The approval card says what is about
to happen; `git diff` says what did.

## A turn that went wrong

1. `git status --short` — see which paths moved.
2. `git diff -- path` — read the change.
3. `git checkout -- path` or `git reset --hard` — revert what you do not want.
4. Say what went wrong in the next message. The transcript still holds the tool
   call that made the change.

For a long task, commit before you start and commit again whenever the diff
reads correctly. Each commit is a checkpoint you can return to.

## Where this is specified

[Streamlining roadmap](../streamlining.md#s2--no-in-app-code-review) records why
the in-app review screen was removed. The API routes it used are still served
and documented in [HTTP API](../reference/http-api.md); nothing in the app calls
them.

## Next

- [Using an editor](editor.md) — open a changed path in your own editor.
- [Approvals](../concepts/approvals.md) — what prompts before a command runs.
