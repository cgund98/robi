# Instructions

This page defines the system prompt sent with every model request. Rendering
lives in `robi-core::prompt`. The sources that fill those blocks live in
`crates/robi/src/prompt/`. The adapter still injects the finished string, as
[providers-streaming.md](providers-streaming.md) describes.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| How the request prepends the string | [providers-streaming.md](providers-streaming.md) |
| Which tools exist | [read-tools.md](read-tools.md), [editing-tools.md](editing-tools.md) |
| Skills, mode prefixes, and a trust decision before reading a project file | [roadmap](../roadmap.md) M5 and M8 |

## Problem

The transcript has no system role, so the model only learns what Robi is, which
tools this session registered, and which project rules apply if something else
says so. That text has several owners: the binary, the user, and the workspace.
A later owner is more specific, and a missing file must not block a turn.

## Decision

`PromptAssembler` holds an ordered list of `PromptSource` values. Each source
returns one block or nothing. `render` joins the blocks that have text. The
default chain, built by `assemble_session` when a session actor starts, is:

1. **Built-in.** Identity, rules, an `<access>` section, and one line per
   registered tool (`name` and `description`). The access section tells the
   model how allow and deny rules rank, and to call `grant` when a tool
   refuses a path. An empty registry says that no tools are registered.
2. **User setting.** The `system_prompt` settings key, when it is non-empty,
   wrapped in `<user_prompt>`.
3. **`~/.robi/system.md`.** The same tag, when the file exists.
4. **`~/.robi/AGENTS.md`.** Wrapped in `<user_agents>`.
5. **Project chain.** From the git root down to the workspace, root first, in
   `<project_agents>`. `AGENTS.override.md` in a directory replaces `AGENTS.md`
   in that directory only. `fallback_files` on `ProjectAgents` adds extra names
   beside those two. The default chain passes an empty list.
6. **Working directory.** The workspace root, in `<cwd>`.

A source is added with `PromptAssembler::source`. A new kind of instruction is
another type that implements `PromptSource`. It does not edit the built-in text.

Each file block is capped at 32 KiB. Over that, the tail is kept and prefixed
with `[earlier instructions truncated]`. The project chain is one block, so a
short budget drops the root file before the closest one.

`SettingsModelSource::model` receives the session tool registry and the
workspace root, assembles this chain, and stores it on `ProviderSettings`. The
provider prepends it as a system message. The transcript does not store it.

Instruction text cannot add a tool or change a path rule. The built-in rules
say so, and the tools do not read the prompt to decide.

## Rejected alternatives

- **A single string constant.** Adding `AGENTS.md` would mean editing the
  constant, and the tool list would drift from the registry.
- **Putting file reads in `robi-core`.** The core renders blocks. Opening
  `AGENTS.md` is I/O, so the sources stay in `crates/robi`.
- **Replacing the built-in prompt with the user file.** The tool list would
  disappear as soon as someone wrote a preamble.

## Failure modes

- A missing file is skipped.
- A file that cannot be read is skipped, and a warning is logged. The turn
  still starts.
- No workspace root skips the project chain and the working-directory block.
  Tests that build a model without a session take that path.
- No home directory skips `~/.robi/system.md` and `~/.robi/AGENTS.md`.
