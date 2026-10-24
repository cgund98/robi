# Using an editor

Robi is a coding assistant you run next to an editor. It talks to a model, edits
files in the workspace, and asks you to approve those edits. Your editor is
where you read the code, jump around, and keep working after Robi has changed
something.

Robi does not replace that editor. It has no buffer, no tabs of open source
files, and no keybindings for writing code. Day-to-day editing stays in the
program you already use.

## Open a changed file

A click on a path name in an edit card follows **Open externally** in
**Settings → General → Files**.

1. Under **Open externally**, put one glob per line. `*` matches any run of
   characters, including `/`. `*.rs` matches every Rust file. An empty list
   opens every path in Robi's file viewer.
2. Under **Open command**, set how a match opens. Leave it empty to use the
   app your system uses for that file.
3. Click the path on an edit card. A path that matches a glob opens outside
   Robi. Any other path opens in the file viewer.

The line Robi passes is the first added or removed line in that card. A click
with no changed line opens the file without a line.

## Point the command at your editor

**Open command** is one line. `{file_path}` is the absolute path.
`{line}` is that 1-based line.

```toml
file_open_command = "zed {file_path}:{line}"
```

That starts `zed` with one argument, the path and line joined by a colon, the
form Zed's CLI expects. A few others:

| Editor | Command |
|---|---|
| Zed | `zed {file_path}:{line}` |
| VS Code | `code --goto "{file_path}:{line}"` |
| Cursor | `cursor --goto "{file_path}:{line}"` |

Double quotes group one argument, so a path with a space stays one argument.
The line is not run in a shell. When the click has no line, Robi drops `{line}`
and a colon immediately before it, so `zed {file_path}:{line}` becomes
`zed /abs/path`.

The same key is `file_open_command` in `~/.robi/config.toml`. The full key list
is in [Configuration](../reference/configuration.md).

Robi waits up to 10 seconds after it starts the program. An editor that is
still running then has opened. A program that cannot start, or that exits
non-zero inside that window, shows the error on the edit card and leaves the
file viewer closed.

## See what changed

Robi is not where you review a change. A turn's edits are on disk as soon as the
tool call runs, so `git diff` is the record:

```sh
git status --short     # which paths this session touched
git diff -- path       # what changed in one file
```

The chat streams every tool call as it happens, so the transcript says what ran
and on which path. To keep or discard work, use git: `git checkout -- path`
throws one file away, and `git stash` before a risky turn gives you a clean undo.

See [Reviewing changes with git](code-review.md), and
[Quickstart](quickstart.md) if you have not run a turn yet.
