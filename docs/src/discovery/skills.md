# Skills discovery

Notes for the skills work in [M8](../roadmap.md). This page is the research.
The decisions are in [design/skills.md](../design/reach/skills.md): which directories
are read, how `@id` loads a skill, and how the model loads one with the
`skill` tool.

Sources were read on 2 October 2026: the [Agent Skills
specification](https://agentskills.io/specification), [OpenCode](https://opencode.ai/docs/skills/)
and [OpenCode v2](https://opencode.ai/v2/docs/skills/), [Claude
Code](https://code.claude.com/docs/en/skills), [Codex](https://developers.openai.com/codex/skills),
[Cursor](https://cursor.com/docs/skills), [Gemini
CLI](https://geminicli.com/docs/cli/skills/), and gopi's
[skills](../../../../gopi/docs/src/concepts/skills.md) page.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| The instruction chain Robi already sends | [instructions.md](../design/core/instructions.md) |
| Modes and which tools they register | [agent-modes.md](../design/core/agent-modes.md) |
| MCP servers | [mcp.md](../design/reach/mcp.md) |
| Plugins | [roadmap](../roadmap.md) M8, still open |
| The settled format, tool, and `@id` behavior | [design/skills.md](../design/reach/skills.md) |

## What a skill is, across these products

A skill is a directory with a `SKILL.md`. The file starts with YAML
frontmatter, then markdown instructions. Optional files sit beside it:
`scripts/`, `references/`, `assets/`. The agent sees a short catalog (name
and description) on every turn. It loads the body only when a skill applies.
Supporting files load later, if the body points at them.

The specification requires `name` and `description`. `name` is 1–64
characters, lowercase letters, digits, and single hyphens, and it matches the
directory name. `description` is 1–1024 characters and says what the skill
does and when to use it. Optional spec fields are `license`, `compatibility`,
`metadata`, and an experimental `allowed-tools`. The body has no required
shape. The spec recommends keeping `SKILL.md` under 500 lines and moving
detail into referenced files.

That split is the whole point. `AGENTS.md` is always in the prompt. A skill
is a procedure that would be wasteful to send on every turn: how to cut a
release, how to review a diff, how this repo's tests are run.

A skill is not a plugin. In every product that stays close to the spec, the
skill adds no tools and grants no extra authority. Scripts run through the
agent's existing shell. gopi states this outright, and Robi's instruction
page already says instruction text cannot add a tool or change a path rule.

## How each product lets someone define one

| Product | Where the user writes it | How they invoke it | How the model loads the body |
|---|---|---|---|
| gopi | `~/.gopi/skills/<name>/SKILL.md`, `<workspace>/.gopi/skills/`, or `instructions.skill_dirs` | No slash command. The model decides, or the user asks in prose | Catalog in the prompt (name, description, path). The model calls `read_file` |
| OpenCode | `.opencode/skills/`, plus `.claude/skills/` and `.agents/skills/`, project and global. v2 also takes extra directories and an HTTP catalog | v2: mention `@skill-id`. The model may also load it | A `skill` tool. The description lists id, name, and description. The result is the body, the base directory, and up to ten supporting paths |
| Claude Code | `~/.claude/skills/`, `.claude/skills/`, enterprise managed settings, plugins, and skills synced from claude.ai. Legacy `.claude/commands/*.md` still works | `/skill-name`, with optional arguments. The model may also load it | The slash command injects the body. Automatic use is a separate load. The directory name is the command; frontmatter `name` is a label, except inside a plugin |
| Codex | `.agents/skills/` from the working directory up to the repo root, `~/.agents/skills/`, `/etc/codex/skills`, and skills bundled with Codex | `$skill-name`, or `/skills`. The model may also load it | Catalog of name, description, and path, capped at 2% of the context window (or 8,000 characters). The full body loads when selected |
| Cursor | `.agents/skills/`, `.cursor/skills/`, the same two under `~/`, and for compatibility `.claude/skills/` and `.codex/skills/` | `/skill-name` in the agent chat. Option-Enter (Alt-Enter) pins the skill as a custom mode for the session | Catalog at startup. The agent decides. A slash attaches the skill to one message |
| Gemini CLI | `~/.gemini/skills/` or `~/.agents/skills/`, and `.gemini/skills/` or `.agents/skills/` in the workspace. Extensions can ship skills | The model calls `activate_skill`. `/skills` lists, enables, disables, links, and reloads. There is no per-skill slash | Name and description in the system prompt. Activation asks the user, then injects the body and the folder listing, and adds the skill directory to the readable paths |

Codex and Cursor also ship a creator the user invokes (`$skill-creator`,
`/create-skill`). The creator asks what the skill does, when it should
trigger, and whether it needs scripts, then writes the directory. Codex adds
Record & Replay: the user demonstrates the workflow and Codex drafts a skill
from the recording. Claude Code's `/run-skill-generator` is a narrower
version of that idea, for how to launch the current project.

## Where they disagree

### Precedence

gopi, OpenCode, and Gemini let the closer, more specific root win. A project
skill overrides a user skill with the same name. Claude Code does the
opposite across levels: enterprise overrides personal, and personal overrides
project. A `deploy` in `~/.claude/skills/` silently beats the one the repo
checked in.

Codex does not override. Two skills with the same name both appear in the
picker.

Robi's instruction chain already treats a later, closer owner as more
specific. A project skill overriding a personal one matches that. A personal
skill quietly hiding the repo's skill does not.

### Who is allowed to invoke

Claude Code splits this into two switches, and Cursor and OpenCode v2 copied
the first:

| Frontmatter | User can type it | Model can load it | Typical use |
|---|---|---|---|
| Default | Yes | Yes | A procedure either side might start |
| `disable-model-invocation: true` | Yes | No | Deploy, commit, send a message. The user picks the moment |
| `user-invocable: false` | No | Yes | Background knowledge. Not a command |

OpenCode v2 uses `metadata.opencode/autoinvoke: false` for the same effect as
`disable-model-invocation`, and that key wins when both are set. Codex puts
the same idea in `agents/openai.yaml` as `policy.allow_implicit_invocation`.
gopi has neither switch: every valid skill is in the catalog.

Cursor and Claude also accept `paths` (globs). The skill is offered only
while the agent is working on matching files. A nested `.cursor/skills/` or
`.claude/skills/` inside a monorepo package is scoped to that directory
without a glob.

### Permission to load

OpenCode treats skill names like other permissions: `allow`, `deny`, or
`ask`, with wildcard patterns, overridable per agent. `deny` hides the skill.
`ask` shows an approval before the body enters the conversation.

Gemini asks on every activation, because activation also grants read access
to the skill directory.

gopi does not ask. It adds the skill roots to the session's read grants, so
reading a body does not raise a card. The body is still untrusted data, the
same as a file or a web page. Project skills are scanned only after the
workspace is trusted.

Claude Code's `allowed-tools` is a different thing: for the turn that invoked
the skill, listed tools skip the permission prompt. That is a skill granting
authority, which the spec marks experimental. Combined with `!`command``
lines, which run a shell command and inline its output before the model sees
the body, a skill can act before anyone reads it.

### Finding and fixing a skill that did not load

gopi skips a file that lacks `name` or `description` and does not say so. The
catalog is built at startup, so a new skill needs a restart.

Claude Code watches the skill directories and picks up `SKILL.md` edits in
the current session. Codex detects changes and falls back to a restart.
Gemini has `/skills reload`.

OpenCode's troubleshooting list is the practical one: exact filename
`SKILL.md`, both frontmatter fields, unique names, and a permission that is
not `deny`. v2 adds a sharper footgun: the id comes from the path, and the
frontmatter `name` is only a display label, so `@git-release` fails when the
folder is `release`.

### Sharing

Local folders are enough for one person and for a repo. Distribution is a
second product in the larger tools: Claude plugins and claude.ai sync, Codex
plugins plus `$skill-installer`, Cursor's team marketplace and "sync skills
for cloud agents", Gemini's `gemini skills install` from a git URL, OpenCode
v2's HTTP catalog. None of that is required to author or use a skill.

## What Robi already has

The prompt assembler is an ordered list of sources. A skill catalog is
another source, the same way gopi inserts one. The roadmap puts that source
in M8, after the instruction chain. Project files still need a trust decision
that does not exist yet; instructions.md leaves that beside skills.

The composer has mode, model, and effort. It has no slash menu, no mention
picker, and no skills section in settings. Settings is a full page with a
section rail and rows inside bordered cards, described in
[visual-style.md](../design/shell/visual-style.md).

The loop already renders tool calls as cards. A load that is a tool call
shows up in the transcript with no new chrome. A load that is a hidden prompt
injection does not.

## A solid UX for Robi

The design in [design/skills.md](../design/reach/skills.md) takes the directory list,
the `@id` mention, and a bundled `create-skill` procedure from this section.
It leaves the settings page and the slash menu for later.

Four surfaces, with files as the only store. A form that becomes the source
of truth will drift from the directory the agent reads, and it will not
round-trip a skill written for another tool.

### 1. Directories people already know

Read the Agent Skills layout, and read the portable directory other tools
already share.

| Scope | Path | When it applies |
|---|---|---|
| User | `~/.robi/skills/<name>/SKILL.md` | Every workspace |
| User, shared | `~/.agents/skills/<name>/SKILL.md` | Every workspace. The directory Codex, Cursor, Gemini, OpenCode, and Goose already use |
| Project | `<workspace>/.agents/skills/<name>/SKILL.md`, walked from the git root down to the workspace | This workspace, after it is trusted |
| Project, Robi-only | `<workspace>/.robi/skills/<name>/SKILL.md` | Same, and wins over `.agents` so a Robi-specific copy can override a shared one |

Also read `.claude/skills/` and `.codex/skills/`, project and user, so a repo
that already has Claude or Codex skills works without a copy. Skip
`.cursor/skills/` unless someone asks: Cursor's `paths`, `icon`, and `color`
fields are harmless to ignore, but its directory is the least shared.

Closer wins, same as the instruction chain. User, then project from the root
downward, then `.robi` over `.agents` in the same directory. The loser stays
visible in settings, marked as overridden, with the path of the winner.
Codex's "show both to the model" makes the catalog lie about which body will
load. Claude's "personal beats the repo" hides the skill the team committed.

A file missing `name` or `description`, or whose `name` does not match its
directory, is listed in settings with the reason. It is absent from the
catalog. gopi's silent skip is acceptable in a terminal and poor in a window
that can show the row.

Watch the directories. The next turn sees an edit. gopi's restart is a
consequence of building the catalog once at process start, not a property of
skills.

### 2. A `skill` tool, and a short catalog

Put name, description, and scope in the prompt. Leave the body out. Cap the
catalog the way Codex does: a fixed budget, shorten descriptions before
dropping entries, and say in the prompt how many were omitted. Settings shows
the same omission, so a missing skill is explainable.

The model loads a body by calling `skill` with the name. The tool result is
the markdown without frontmatter, the skill directory, and the names of
supporting files, not their contents. The model reads those with `read_file`
when the body says to. That is OpenCode's load, and it is a better fit than
gopi's `read_file` on a path buried in the catalog:

- The transcript already has a card for a tool call. The card is the receipt:
  the name, the one-line description, and the path. The body stays in the
  model context and out of the default row.
- The tool can refuse a name the catalog does not list, including one the
  user marked manual-only.
- The tool does not grant new paths. Supporting reads use the grants the
  session already has. Skill roots are readable without a card, as in gopi,
  and nothing outside them opens because a skill said so.

Recognize `disable-model-invocation: true` (and OpenCode's
`metadata.opencode/autoinvoke: false`) so an imported skill does not start
running on its own. Those skills stay in the `/` menu and out of the catalog.

Ignore, and do not implement, the fields that turn a skill into a policy
change: `allowed-tools`, `disallowed-tools`, `hooks`, `context: fork`,
`model`, `effort`, and Claude's `!`command`` shell injection. A skill that
needs a command says so in the body, and `shell` applies the sandbox and the
approval rules it already has. Document that in the skill page when it
exists, so a Claude skill that depends on those fields fails in an obvious
way rather than half-running.

`user-invocable: false` is worth honoring if the field is present: hide it
from the menu, keep it in the catalog. It is a small behavior and it matches
skills written as reference rather than as commands.

`paths` can wait. Scoping a nested `.agents/skills/` to its directory, the
way Cursor and Claude scope a nested skill, covers the monorepo case without
a glob language. Glob scoping is a later addition if directory scope is not
enough.

### 3. The composer menu

`/` in the composer opens a filterable list: name, description, and a scope
mark (yours, or this workspace). Choosing one inserts `/name` into the draft.
Sending that message invokes the skill as a user action: the body is loaded
for that turn, and the transcript shows the same card the tool would. Text
after the name is the argument, appended to the loaded body. Named
placeholders and `$ARGUMENTS[N]` can wait; Claude's substitution table is
larger than the feature.

The menu is also where manual-only skills live, since the model cannot see
them. An empty menu says where a skill file goes and links to the settings
section. That is the gap in gopi: the only way to learn the catalog is to
ask the model or read the prompt.

Pinning a skill for the whole session, Cursor's custom-mode gesture, is a
later choice. One-shot invocation matches "this message is a release" better
than "every message is a release". A pin duplicates modes, which Robi already
has.

### 4. Settings, as a list of files

A **Skills** section on the existing settings page, in the same row-and-card
layout as the other sections. Two groups, **This workspace** and **Yours**.

Each row shows the name, the description, the scope, and the path. An
overridden or invalid skill says why. A switch disables a skill without
deleting it; Codex does this in `config.toml`, Gemini does it with
`/skills disable`. The switch is per skill name and scope, stored in Robi's
settings, and it applies on the next turn.

**New skill** asks for a name, a description, and a scope, writes a stub
`SKILL.md`, and reveals the file. The stub is the spec's minimum, plus a
short "when to use this" section. Editing stays in the user's editor. Robi
does not grow a markdown IDE for this.

**Open** reveals the directory. There is no installer, catalog URL, or
marketplace in the first version. Someone who wants a shared skill commits
`.agents/skills/` or copies a directory into `~/.agents/skills/`.

A built-in `create-skill`, invocable from the menu and hidden from automatic
use, is the in-chat path Codex and Cursor already taught people. It writes
the same stub the button writes, after asking what the skill does and when
it should trigger. Record & Replay is a different feature: it needs a trace
of the session and a judgment about which steps were the procedure. Leave it
until skills themselves are in use.

### Trust and the transcript

Project skills are repository content. They stay out of the catalog until the
workspace is trusted, the same gate instructions.md already defers. Until
that gate exists, settings lists them as present and not loaded, with the
reason. User skills under `~/.robi` and `~/.agents` load without that gate;
they are the user's own files.

Loading a skill does not approve a later `shell` or `write_file`. The card
for the load and the card for the command stay separate, so a skill that says
"run this installer" still looks like an installer.

Subagents get the catalog only if their mode is allowed to call `skill`.
Explore should not: it is a search, and a project skill is a prompt. That
choice belongs in the modes page when skills are designed.

Headless mode has no menu. The catalog and the `skill` tool are the feature
there. The menu and the settings list are the desktop presentation of the
same list.

## What to leave for later

- Plugin marketplaces, HTTP catalogs, and install-from-git.
- Shell injection and tool pre-approval inside a skill.
- Per-skill model and effort overrides.
- Glob `paths`, session pinning, and argument templates beyond "the rest of
  the line".
- Record & Replay.
- A skill changing the sandbox, the protected paths, or the tool registry.

## Open questions

- Is `.claude/skills/` plus `.codex/skills/` enough compatibility, or do
  people also keep skills only under `.opencode/skills/` and `.cursor/skills/`?
- Does disabling a skill hide it from the menu, or only from the model?
- When the catalog budget drops a skill, is the settings row enough, or does
  the composer menu need the same warning?
- Should `create-skill` be allowed to write `~/.robi/skills/` from a chat
  without an approval card? Writing outside the workspace is exactly what the
  editing tools pause for.
- Do nested project skills (a package's `.agents/skills/`) enter the catalog
  at session start, or only after a tool touches that directory, as Claude
  Code does?
