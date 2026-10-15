# Using an editor

Robi is a coding assistant you run next to an editor. It talks to a model, edits
files in the workspace, and asks you to approve those edits. Your editor is
where you read the code, jump around, and keep working after Robi has changed
something.

Robi does not replace that editor. It has no buffer, no tabs of open source
files, and no keybindings for writing code. The review screen and the docs
viewer are for looking at a change or a markdown page inside the app. Day-to-day
editing stays in the program you already use.

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

## Keep the rest of the loop in Robi

Approve or reject the edit in the chat before you rely on the file in your
editor. The review screen still lists every path the session changed, and
**Approve** or **Reject** there is what keeps or discards the change. Opening
the file elsewhere does not approve it.

See [Code review](code-review.md) for that screen, and
[Quickstart](quickstart.md) if you have not run a turn yet.
