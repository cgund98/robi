# Agent modes

A session runs in one mode. The mode chooses the tool registry and the last
block of the system prompt. It also chooses which model and effort that turn
uses. The loop in `robi-core` does not know the mode.

## What this page does not cover

| Topic | Where it belongs |
|---|---|
| Plan checklist UI and re-feeding a plan into a later turn | [roadmap](../roadmap.md) F5.2 |
| `tasks` and web tools | [roadmap](../roadmap.md) F5.3 and M8 |
| `delegate` | [subagents.md](subagents.md) |
| How the prompt is assembled aside from the mode block | [instructions.md](instructions.md) |
| Approval and path rules | [read-tools.md](read-tools.md) |

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
| `read_file`, `list_dir`, `find`, `grep`, `grant` | yes | yes | yes |
| `shell` | no | yes | yes |
| `write_file`, `edit_file`, `delete_file` | no | no | yes |
| `write_plan` | no | create or update | update only |
| `delegate` | no | no | yes |

`register_tools_for_mode` builds that set. The provider is offered the same
registry, so the model cannot call a tool the mode did not register.

### Prompt

`assemble_session` appends a `<mode>` block after `<cwd>`. The block states
the job:

- **Ask** answers from the workspace with read-only tools. It does not edit
  files or run commands.
- **Plan** explores with read tools and a sandboxed shell, then saves with
  `write_plan`. It does not edit project files. Saving a plan does not apply
  it. The user applies it by switching to Agent mode.
- **Agent** reads, edits, and runs commands. It applies changes with the edit
  tools. A search across more than a couple of files goes to `delegate` with
  mode `explore`. A single known file stays `read_file`. `delegate` with mode
  `general` is for a task that needs a command. When the user asks for a plan,
  it does not start the work. It asks them to switch to Plan mode. It revises
  an existing plan under `.robi/plans` with `write_plan` and that path.

The built-in identity no longer says the assistant always edits. The tool
list still comes from the registry.

### `write_plan`

Plans live at `.robi/plans/<slug>-<uuid>.md`. The body is the markdown plan.
`todos` are written as YAML frontmatter above it. Each todo has an `id`,
`content`, and `status` of `pending`, `in_progress`, `completed`, or
`canceled`. At most one todo is `in_progress`. Omit `path` to create a file.
Pass `path` to overwrite an existing markdown file in that directory. Agent
mode requires `path` and refuses a file that does not exist.

A plan write does not pause for approval. The path must stay inside
`.robi/plans` and end in `.md`. A secret-file deny still applies. If the
workspace root already has a `.gitignore`, the tool appends `.robi/plans`
once. It does not create a `.gitignore`. The write records a file-change
baseline, the same way `write_file` does.

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
- `write_plan` refuses a path outside `.robi/plans`, a non-markdown path,
  and, in agent mode, a missing file.
- A missing `.gitignore` is left missing. A second save does not append the
  ignore line again.
