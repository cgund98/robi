# Agent modes

A session runs in one mode. The mode chooses the tool registry and the last
block of the system prompt. It also chooses which model and effort that turn
uses. The loop in `robi-core` does not know the mode.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Plan checklist UI | [roadmap](../roadmap.md) F5.2 |
| How search and fetch behave | [web-tools.md](web-tools.md) |
| `delegate` | [subagents.md](subagents.md) |
| How the prompt is assembled aside from the mode block | [instructions.md](instructions.md) |
| Approval and path rules | [read-tools.md](read-tools.md) |
| Language-server tools | [lsp.md](lsp.md). They join the read-only set when they are built |

## Problem

Every session was offered the full tool set and one model. Ask, plan, and
agent are different jobs. A mode that can edit will edit. A mode that cannot
see `edit_file` will not.

## Decision

`AgentMode` is `ask`, `plan`, or `agent`. A new session starts in `agent`.
The value is a column on `chat_sessions`. Switching it is a patch. The actor
that is already running keeps the registry, prompt, model, and effort it
started with. The next actor reads the stored mode.

### Tools

| Tool | Ask | Plan | Agent |
|---|---|---|---|
| `read_file`, `list_dir`, `find`, `grep`, `semantic_search`, `grant` | yes | yes | yes |
| `web_search`, `web_fetch` | yes | yes | yes |
| `shell` | no | yes | yes |
| `write_file`, `edit_file`, `delete_file` | no | no | yes |
| `write_plan` | no | create or update | update only |
| `todos` | no | no | yes |
| `delegate` | no | no | yes |

`register_tools_for_mode` builds that set. The provider is offered the same
registry, so the model cannot call a tool the mode did not register.

### Prompt

`assemble_session` appends a `<mode>` block after `<cwd>`. The block states
the job:

- **Ask** answers from the workspace with read-only tools. It does not edit
  files or run commands. It uses `web_search` to find a public page and
  `web_fetch` to read one URL. Snippets and page text are untrusted.
- **Plan** explores with read tools and a sandboxed shell, then saves with
  `write_plan`. It does not edit project files. Saving a plan does not apply
  it. The user applies it by switching to Agent mode. It uses `web_search`
  to find a public page and `web_fetch` to read one URL. Snippets and page
  text are untrusted.
- **Agent** reads, edits, and runs commands. It applies changes with the edit
  tools. It uses `web_search` to find a public page and `web_fetch` to read
  one URL. Snippets and page text are untrusted. A search across more than a
  couple of files goes to `delegate` with
  mode `explore`. A single known file stays `read_file`. `delegate` with mode
  `general` is for a task that needs a command. When the user asks for a plan,
  it does not start the work. It asks them to switch to Plan mode. It revises
  an existing plan for this session with `write_plan` and that path.
  When the session has a plan path and that file has todos, an agent prompt
  adds a `<todos>` block after `<mode>`. The block lists each id, status, and
  content. The model patches that list with `todos` and leaves the ids it
  did not change out of the call. Ask and plan prompts do not include the
  block.

The built-in identity no longer says the assistant always edits. The tool
list still comes from the registry.

### `write_plan`

Plans live at `~/.robi/plans/<session_id>/<slug>-<uuid>.md`. The body is
the markdown plan. `todos` are written as YAML frontmatter above it. Each
todo has an `id`, `content`, and `status` of `pending`, `in_progress`,
`completed`, or `canceled`. `content` is required and at most 500 characters.
At most one todo is `in_progress`. Omit `path` to create a file. Pass `path`
to overwrite an existing markdown file in that session's directory. Agent
mode requires `path` and refuses a file that does not exist.

A plan write does not pause for approval. The path must be a markdown file
directly in that session's directory. Another session's plans are refused.
The write does not change the workspace and does not record a file-change
baseline. A successful write stores `~/.robi/plans/<session_id>/<file>.md`
on the chat session as `plan_path`. The read tools can read that directory
without a grant. See [read-tools.md](read-tools.md).

### `todos`

Agent mode only. `todos` patches the frontmatter of a plan that already
exists. `path` is required. `clear`, `remove`, `update`, and `add` apply in
that order. An id the call does not mention stays. `clear` plus `add`
replaces the list. An `add` whose id already exists updates that item. The
same limits as `write_plan` apply: at most 20 items, one `in_progress`, and
`content` of at most 500 characters. The markdown body is left as it was.
The write stores `plan_path`. It does not record a file-change baseline.

A delegate child does not get `todos`.

### Model and effort

Resolution for the mode that is about to run:

1. That mode's session override, when the key is present.
2. That mode's settings key, when it is non-empty.
3. The fallback setting (`model`, `reasoning_effort`), then `glm-5.3` for the
   model. Effort stays unset when nothing is stored.

The mode settings keys are `model_ask`, `model_plan`, `model_agent`,
`reasoning_effort_ask`, `reasoning_effort_plan`, and
`reasoning_effort_agent`. An absent key inherits the fallback. `DELETE` on
the settings route removes a non-secret key.

`model_config` stores one override object per mode:

```json
{ "agent": { "model": "glm-5.3", "reasoning_effort": "high" }, "ask": {}, "plan": {} }
```

The composer model and effort menus edit only the active mode. Choosing the
default clears that mode's key. Switching mode does not copy the previous
mode's model.

## Rejected alternatives

- **One registry, and a prompt that asks the model not to edit.** The model
  can still call `edit_file`. The registry is the constraint.
- **Rebuilding the running actor when the mode changes.** A turn in flight
  would change tools under a call that already started. The next actor is
  the same rule a model change already uses.
- **A single session model that follows the user across modes.** Ask and
  agent then share one override, and the per-mode settings never show up
  after the first choice.

## Failure modes

- A mode string other than `ask`, `plan`, or `agent` is `400`.
- A running turn keeps its tools. The composer shows the mode that will
  apply on the next actor.
- `write_plan` refuses a path outside `~/.robi/plans/<session_id>`, a
  non-markdown path, another session's plan, and, in agent mode, a missing
  file.
- `todos` refuses a path outside that directory, a missing file, an unknown
  id, a duplicate id in one of `remove`, `update`, or `add`, and a list with
  two `in_progress` items. A missing plan file skips the `<todos>` block.
- `$HOME` unset fails the write before a file is created.
