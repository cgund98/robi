# Skills

A **skill** is a directory containing a `SKILL.md`. The file starts with YAML
frontmatter and the body is markdown instructions. Skills are how you give Robi a
reusable procedure — a release checklist, a house style, an interview flow —
without typing it every time.

```
~/.robi/skills/land-it/SKILL.md
```

The **id** is the directory name, and it must be lowercase letters, digits, and
single hyphens (1–64 characters). A category folder is allowed: the skill at
`skills/shipping/land-it/SKILL.md` still has the id `land-it`.

## Frontmatter

| Field | Effect |
|---|---|
| `description` | The catalog text. When absent, the first non-empty paragraph is used (cut at 1024 characters); if neither exists, the skill is skipped. |
| `name` | The display label in the composer. The id stays the directory name. |
| `disable-model-invocation: true` | Hidden from the model's catalog but still loadable when you mention it. |
| `user-invocable: false` | Hidden from the `@` menu but the model can still load it. |

Other frontmatter is ignored. A `` !`command` `` line is treated as markdown;
nothing in a skill is executed at load time.

## Where skills are found

The scan reads `SKILL.md` at any depth under these roots, home first and then each
directory on the workspace chain (git root down to the workspace root):

- `~/.claude/skills`, `~/.codex/skills`, `~/.cursor/skills`,
  `~/.opencode/skills` and `~/.config/opencode/skills`, `~/.agents/skills`,
  `~/.robi/skills` — and the same names under each workspace-chain directory.

Later wins, and within one directory `.robi` beats `.agents` beats tool-specific
names, so a project skill overrides a home skill with the same id. The scan runs
when a session's actor starts, so a file you save mid-turn is visible on your
next message.

## Using a skill

- **You** type `@` at the start of the composer (or after a space) to open the
  menu, and pick a skill. Selecting inserts `@id `. On send, every `@id` is
  resolved and loaded; an unknown `@word` stays as plain text.
- **The model** loads a skill with the `skill` tool, taking the id. It is
  registered in every mode and needs no approval.

A loaded skill appears in the transcript as **Using {id}**, with the description,
path, and body on the detail view. The provider receives the body in a
`<skill name=… directory=…>` block.

## The catalog

Robi inserts a catalog of model-loadable skills into the prompt — one line per
skill, `id: description`, descriptions cut to 256 characters and the whole block
capped at 8 KiB (overflow ends with `[N skills omitted]`). A skill marked
`disable-model-invocation` is left out of this block. The catalog text is
untrusted, like any loaded content.

## Bundled skills

Two skills ship in the binary and appear in the catalog and the `@` menu on
every session, ahead of the home roots, so a file with the same id replaces
them.

- **`create-skill`** writes a new skill for you: it asks whether the model may
  load it, where to put it (default `~/.robi/skills/<id>/SKILL.md`, or the
  workspace's `.robi/skills/`), and confirms the id, description, and body before
  writing.
- **`configure-mcp`** walks you through adding an MCP server: it asks which tool
  you want to connect, whether the config is global (`~/.robi/mcp.json`) or for
  this workspace (`<workspace>/.robi/mcp.json`), the transport and its settings,
  and any secret references, then writes the entry and tells you to restart the
  app. See [MCP](../concepts/mcp.md).

In ask or plan mode both show the draft and tell you to switch to agent mode,
because those modes cannot write.

## Where this is specified

Discovery roots, the catalog, the load path, and the bundled skills are in
[Skills](../design/reach/skills.md). The reference scan is in
[Skills discovery](../discovery/skills.md).
