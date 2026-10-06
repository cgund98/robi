# Robi documentation

Robi is a desktop coding assistant: a Rust core that runs an agent loop against a
local workspace, and a Solid front end that renders the conversation.

New here? Start with the [Quickstart](guides/quickstart.md).

## Guides

Task-focused pages. Each one gets you to a result.

- [Quickstart](guides/quickstart.md) — run the app, the local API, and your first message.
- [Releases](guides/releases.md) — how a push to `main` opens the version bump.

## Concepts

Why Robi behaves the way it does.

- [How Robi works](concepts/how-robi-works.md) — the crate layers, the loop, and the transcript.
- [Approvals](concepts/approvals.md) — per-call approval, the policy floor, and grants.
- [Sessions and persistence](concepts/sessions-and-persistence.md) — saved chats, the store, and settings.
- [Skills](concepts/skills.md) — reusable instructions Robi loads on demand.
- [Subagents](concepts/subagents.md) — `explore` and `general` children.
- [MCP](concepts/mcp.md) — tools that come from outside Robi.

## Reference

Lookups. Exact keys, names, and paths.

- [Tools](reference/tools.md) — every built-in tool and when it pauses.
- [Configuration](reference/configuration.md) — keys in `config.toml` and `secrets.toml`.
- [File locations](reference/file-locations.md) — the `~/.robi` layout.
- [HTTP API](reference/http-api.md) — the `robi-api` routes and the event stream.

## Project internals

Everything below this point is **for contributors**. These pages are the plan of
record and the design decisions behind the code, not a description of behavior a
user sees. Read them to build a milestone; read a Concept or Reference page to
use the app.

- [Roadmap](roadmap.md) — what Robi ships, in what order, and the open decisions.
- **Design docs** — one page per subsystem, grouped by the module it describes:
  [Core](design/core/agent-loop.md), [Providers](design/providers/providers-streaming.md),
  [Shell](design/shell/frontend-layout.md), [Persistence](design/persistence/persistence.md),
  [Tools](design/tools/read-tools.md), [Review](design/review/code-review.md),
  [Intelligence](design/intelligence/lsp.md), [Reach](design/reach/mcp.md),
  [Compression](design/compression/shell-output.md).
- **Discovery notes** — research that fed a design, kept as an appendix:
  [Skills discovery](discovery/skills.md).

## How these docs are organized

Guides teach, Concepts explain, and Reference states facts — one job per page.
Internals are separate so the design docs can stay spec-shaped (problem,
decision, rejected alternatives, interfaces, failure modes) without dressing up
as user documentation. Every page in this book is listed in `SUMMARY.md`; a page
that is not listed does not render.
