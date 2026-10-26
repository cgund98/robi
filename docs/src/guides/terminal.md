# The terminal app

`robi` is the same agent as the desktop app, in a terminal. Same sessions, same
tools, same settings; it links the same crate and builds the same state the local
API builds. It does not talk to the API over HTTP and opens no port.

## Install

Download the tarball for your machine from the
[releases page](https://github.com/cgund98/robi/releases):

| Machine | Asset |
|---|---|
| Apple Silicon macOS | `robi-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| x86_64 Linux | `robi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |

```sh
tar -xzf robi-vX.Y.Z-aarch64-apple-darwin.tar.gz
sudo install -m 0755 robi-vX.Y.Z-aarch64-apple-darwin/robi /usr/local/bin/robi
```

The macOS binary is not notarised, so a downloaded copy is quarantined and the
system refuses to run it. Clear the flag once:

```sh
xattr -d com.apple.quarantine /usr/local/bin/robi
```

The Linux tarball is built on Ubuntu 22.04, so it needs glibc 2.35 or newer:
22.04 and 24.04 both run it.

## What Linux needs

The default sandbox is `bubblewrap`. Install it, or every sandboxed command is
refused:

```sh
sudo apt install bubblewrap
```

The `container` sandbox is separate and optional. It needs a Docker daemon and
the `sandbox_mechanism = "container"` setting; without either, the default
mechanism runs and the container setting is not used.

## Run it

```sh
robi            # the current directory
robi ~/code/app # a different workspace
```

The path decides which sessions you see. A path with no workspace behind it is a
question, not a guess: `y` creates a workspace there, `n` opens the workspace
switcher.

```
robi --help      # usage
robi --version   # robi X.Y.Z
```

## Which database it opens

`robi` opens the desktop app's database, so the sessions you see are the sessions
the app shows:

- A debug build, which is what `cargo run` from a checkout gives you, opens the
  app's **dev** database (`~/Library/Application Support/com.github.cgund98.robi/dev/robi.db`
  on macOS), the same one `pnpm tauri dev` uses.
- A release build opens the app's own database
  (`~/Library/Application Support/com.github.cgund98.robi/robi.db`).

`ROBI_DATABASE_URL` overrides both:

```sh
ROBI_DATABASE_URL=sqlite:///Users/you/.robi/sessions.db?mode=rwc robi
```

Settings, secrets, logs, and the UI state live under `~/.robi`, the same
directory the app uses. See [File locations](../reference/file-locations.md).

## Two terminals

Two terminals in one workspace are fine, and so is one beside the desktop app.
Nothing is locked at startup: a lock covers one session, and it is taken when
that session is used.

What that means in practice, for a chat another process has open:

- It renders. Text, tool names, arguments, and statuses are in the chat database.
  Each tool result comes back as `open in another process (pid 4812)` instead,
  because the bytes are in that session's own blob file and only one process may
  open it.
- Sending is refused, with the pid in the footer and your draft left in place.
  Approving a tool call is refused the same way.
- Deleting it is refused.
- A turn that pauses for approval keeps the session until you answer it or quits,
  so the other process cannot resume a call you are still looking at.

Work on a different chat in each terminal, or take turns. Two processes editing
the same workspace file are safe: the write tools take a per-file lock and wait
up to ten seconds for the other one, then refuse with `locked` rather than
overwrite a change they did not see.

## Logs

`~/.robi/logs/`, one file per start, the same directory the app writes to. The
log layer never writes to stderr: a tracing line there would land in the middle
of the frame the terminal is drawing. A fatal error that stops the process
before the frame exists still prints one line there, because nothing else is
watching.
