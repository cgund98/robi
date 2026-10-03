# Skills

A skill is a directory with a `SKILL.md`. The prompt lists the name and
description. The body loads when the user names it with `@id` in the composer,
or when the model calls the `skill` tool. A bundled `create-skill` procedure
interviews the user and writes the directory. This page is the design for skills
in [M8](../../roadmap.md). The research behind it is
[discovery/skills.md](skills.md).

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| The rest of the system prompt | [instructions.md](../core/instructions.md) |
| Which modes register the tool | [agent-modes.md](../core/agent-modes.md). This page names the set |
| Child agents | [subagents.md](../core/subagents.md). They do not receive the catalog or the tool |
| Path grants for ordinary workspace reads | [read-tools.md](../tools/read-tools.md) |
| A settings screen, an installer, or a skill editor | Later. Files are the store |

## Problem

The same procedure gets pasted into chat, or it grows inside `AGENTS.md` and
is then paid for on every turn. A release checklist, a test recipe, a review
pass. Those belong in a file the model can open when the task matches, and
that the user can name when they already know which one applies.

People already keep those files for other agents, under `.claude/skills`,
`.codex/skills`, `.agents/skills`, and the directories those tools share.
Robi has to read those trees, at home and in the workspace, and its own
`.robi/skills` beside them.

## Decision

### Where skills are read

A skill is a directory whose direct file is `SKILL.md`. Roots are scanned for
that filename at any depth, so a category folder such as
`skills/shipping/land-it/SKILL.md` counts. The id is the directory that
contains `SKILL.md` (`land-it`), not the category above it.

Each scope has the same root names. Home is `~/`. The workspace chain is the
git root down to the workspace root, the same walk as project `AGENTS.md`.
No git root scans the workspace directory only.

| Order | Home | Each directory on the workspace chain |
|---|---|---|
| 1 | `~/.claude/skills` | `.claude/skills` |
| 2 | `~/.codex/skills` | `.codex/skills` |
| 3 | `~/.cursor/skills` | `.cursor/skills` |
| 4 | `~/.opencode/skills`, then `~/.config/opencode/skills` | `.opencode/skills` |
| 5 | `~/.agents/skills` | `.agents/skills` |
| 6 | `~/.robi/skills` | `.robi/skills` |

Later wins. Home is applied first, then each directory from the git root
toward the workspace. A project skill overrides a home skill with the same
id. Inside one directory, `.robi/skills` overrides `.agents/skills`, which
overrides the tool-specific roots. Two ids in the same root: the shallower
path wins, and the other is skipped with a warning.

A direct child of a root may be a symlink to a directory. `SKILL.md` is read
from that target. Supporting files are listed from the target and must stay
inside it after canonicalization. A symlink that leaves the target is left
out of the file list.

The scan runs when a session actor starts, which is every user message. A
file saved during a turn is visible on the next message.

### What makes a valid skill

`SKILL.md` starts with YAML frontmatter. The id is the directory name. It
must match `^[a-z0-9]+(-[a-z0-9]+)*$` and be 1–64 characters. A directory that
fails that check is skipped.

| Field | Effect |
|---|---|
| `description` | Catalog text. Required in the usual file. When it is absent, the first non-empty markdown paragraph is used, cut at 1,024 characters. A file with neither is skipped |
| `name` | Display label in the composer menu. The id stays the directory name. `@` uses the id |
| `disable-model-invocation: true` | Left out of the model catalog. `@id` still loads it |
| `metadata.opencode/autoinvoke: false` | Same as `disable-model-invocation: true`. Either one is enough |
| `user-invocable: false` | Left out of the composer menu. The model can still load it. A typed `@id` still loads it |

Other frontmatter is kept in the file and ignored. That includes
`allowed-tools`, `disallowed-tools`, `hooks`, `context`, `model`, `effort`,
`paths`, `license`, and `compatibility`. A `` !`command` `` line in the body
is markdown. Robi does not run it while loading the skill.

A skipped file is logged. The turn still starts.

### The catalog

`SkillCatalog` implements `PromptSource`. `assemble_session` inserts it after
`<cwd>` and before `<mode>`. The block is omitted when the model has nothing
to choose.

```text
<skills>
git-release: Create consistent releases and changelogs
go-tests: Run and interpret Go tests in this module.
</skills>
```

One line per skill the model may load: `id: description`. Descriptions are
cut at 256 characters. The block stops at 8 KiB. Skills past the budget are
dropped from the end of the precedence order (home and broader roots first)
and the block ends with `[N skills omitted]`.

The text is untrusted, the same way a project `AGENTS.md` is untrusted.
Loading a skill does not add a tool, change a path rule, or approve a later
call.

The `skill` tool description says to call `skill` before following a listed
id, and that an `@id` already in the user message is loaded.

### The `skill` tool

Registered in ask, plan, and agent. `Concurrent`. It does not ask for
approval. Explore and general children do not get it, and their prompts do
not include the catalog.

The argument is `name`, the id.

A known, model-invocable id returns:

```json
{
  "name": "git-release",
  "description": "Create consistent releases and changelogs",
  "directory": "/abs/path/to/git-release",
  "files": ["scripts/changelog.ts", "references/release-policy.md"],
  "body": "markdown without frontmatter"
}
```

`files` is at most ten paths relative to the skill directory, files only,
plus `"truncated": true` when more exist. `body` is capped at 32 KiB. Over
that, the head is kept and the result says the tail was cut.

An unknown id, or an id with `disable-model-invocation`, is a tool error.
The error for a manual-only id tells the model to ask the user to mention
`@id`, and not to carry the procedure out on its own.

Supporting reads use `read_file`. Each scanned skill directory is a read
allow for the session, including a directory outside the workspace. The
allow covers that directory and its children. It does not cover writes, and
it does not cover a sibling of the directory. The `..` deny still applies
everywhere else.

### `@id` in the composer

`@` at the start of the composer, or after whitespace, opens a menu of
user-invocable skills. The filter is the text after `@`, matched against the
id, the display label, and the description. A row shows the label (or the id
when they are the same), the description, and whether it comes from home or
this workspace. A manual-only skill is in that menu and marked so. Choosing a
row inserts `@id` and a trailing space. The draft is plain text. The menu is
a Radix popover, styled with the same tokens as the mode menu.

The list comes from `GET /api/v1/workspaces/{id}/skills`. Each entry is `id`,
`label`, `description`, `scope` (`user` or `project`), `model_invocable`, and
`path`. The scan is the same one the actor uses.

On send, the instruction string is unchanged. The handler resolves every
`@id` token that names a skill, including one hidden from the menu. Each
match is loaded once, in order of appearance. The user message stores the
typed text in `content` and the loads on `skills`:

```json
{
  "id": "git-release",
  "description": "Create consistent releases and changelogs",
  "directory": "/abs/path/to/git-release",
  "files": ["scripts/changelog.ts"],
  "body": "markdown without frontmatter"
}
```

`Message.skills` defaults to empty. Old rows still deserialize. The body
column is already the JSON of `Message`, so this needs no migration.

The provider sends the user text, then one block per load:

```text
<skill name="git-release" directory="/abs/path/to/git-release">
markdown body

Files: scripts/changelog.ts
</skill>
```

The transcript shows the typed text, then one quiet row per load: **Using
git-release**. The row opens onto the description, the path, and the body.
An unknown `@word` stays in the text. The send still succeeds. A skill that
fails to read is omitted from `skills` and a warning is logged. The mention
stays in the text.

Headless input is the same string. There is no menu. `@id` still loads.

### Creating a skill

Robi ships one bundled skill, `create-skill`. It is in the catalog and in the
`@` menu on every session, ahead of the home roots, so a file with the same
id in `~/.robi/skills` or the workspace replaces it. The body is compiled
into the binary. Nothing is written until the user agrees to a draft.

The user starts it with `@create-skill`, or by asking for a skill. The
description tells the model to load it when the user wants to create or
update one. The procedure is:

1. If this conversation already contains the workflow, take the steps, the
   commands, and the corrections from it. Otherwise ask what the skill should
   do, when it should trigger, and what the result looks like.
2. Ask whether the model may load it on its own. The default is yes. A no
   sets `disable-model-invocation: true`.
3. Ask where it lives. The default is home, `~/.robi/skills/<id>/SKILL.md`.
   This workspace is `<workspace>/.robi/skills/<id>/SKILL.md`. The id is the
   directory name, and it has to match the id rule above.
4. Confirm the id, the description, and the body before writing. The
   description states what the skill does and the phrases that should trigger
   it. The body is the steps. Scripts, references, and assets are added only
   when the user asked for them.
5. Write `SKILL.md`. An id that already exists is left in place until the
   user agrees to replace it. Ask and plan cannot write files, so those modes
   show the draft and tell the user to switch to agent. A home path is
   outside the workspace, so the write pauses on the approval card the edit
   tools already use. A workspace path follows the same write rules as any
   other project file.

The reply names the path and says `@id` works on the next message. The scan
for the current turn already ran.

Updating a skill is the same procedure. The model reads the existing
`SKILL.md`, applies the change, and writes it back after the user confirms.

## Rejected alternatives

- **Catalog only, and `read_file` for the body.** gopi does this. The load
  is a generic read, the path has to be in the prompt, and `@id` cannot load
  a manual-only skill the model is forbidden to open.
- **`/` as the mention.** Claude and Cursor use it. This composer uses `@`,
  which is the mention OpenCode and ChatGPT already teach, and it leaves `/`
  free for a later command.
- **Hoping the model calls `skill` when it sees `@id`.** A mention the model
  ignores never loads. The host loads it while accepting the message.
- **Putting the body into `content`.** The stored user text would no longer
  be what was typed, and copying the message would copy the skill.
- **A synthetic assistant tool call.** The transcript would claim the model
  asked for a load the user asked for.
- **Personal skills overriding project skills.** Claude Code does this. A
  home copy would hide the skill the repository committed. The instruction
  chain already lets the closer file win.
- **Showing both copies to the model.** Codex keeps duplicate names in the
  picker. The catalog would not say which body `skill` returns.
- **Running `allowed-tools` or `` !`command` `` at load time.** Those grant
  authority and execute shell before anyone has read the skill. A command in
  the body goes through `shell` and its approval rules.
- **A file watcher.** The next actor rescans. A watcher duplicates that for
  the interval between messages.
- **Flat `skills/review.md` and `.claude/commands/*.md`.** Those are command
  files. This page loads `SKILL.md` directories.
- **A settings form that creates the file.** The interview is a skill, the
  same way Codex, Claude, Cursor, and Gemini do it. A form would be a second
  editor for the same directory.
- **Recording the screen.** Codex drafts a skill from a captured workflow.
  That needs a recorder. The conversation is the source this procedure has.
- **An eval harness inside `create-skill`.** Claude's plugin runs the new
  skill against test prompts and tunes the description. The first procedure
  stops at a file the user has confirmed.

## Failure modes

- No home directory skips the home roots. The workspace chain still runs.
- A root that does not exist is skipped.
- A `SKILL.md` that cannot be read or whose frontmatter does not parse is
  skipped. A warning is logged. The turn still starts.
- A catalog over 8 KiB drops entries and says how many. `@id` can still name
  a dropped skill, and `skill` can still load it. The budget limits the
  prompt, not the set of ids.
- A body over 32 KiB is cut. The tool result and the `<skill>` block both say
  so.
- A mention of an id that lost a precedence fight loads the winner.
- A child agent does not see the parent's `<skill>` blocks. The parent copies
  what matters into the `delegate` task.
- `create-skill` in ask or plan does not write. The draft stays in the reply.
- A rejected approval card leaves the skill directory unwritten. The user can
  approve a later attempt.

## Testing

`cargo test -p robi-core` stays free of disk. The catalog source and the
scanner live in `crates/robi` and use a temporary directory. Tests cover
precedence (home loses to the workspace, `.claude` loses to `.agents`,
`.agents` loses to `.robi`), a manual-only id absent from `<skills>` and
present for `@`, a `user-invocable: false` id absent from the menu route and
present in `<skills>`, a typed `@id` stored on `Message.skills`, an
unknown `@word` left as text, and a bundled `create-skill` that a home or
workspace file of the same id replaces.
