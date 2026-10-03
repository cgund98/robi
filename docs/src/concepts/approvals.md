# Approvals

An agent that can edit files and run commands needs a boundary. Robi's boundary
is three independent gates that compose, plus one invariant that ties them to the
conversation.

## The three gates

1. **Per-call approval.** Each tool decides from its own arguments, with a
   side-effect-free `requires_approval`. It returns `AllowImmediately` or
   `NeedsApproval` — never "deny". This is what makes "read inside the workspace"
   free and "read outside it" a prompt.
2. **Policy floor.** A set of globs the model cannot talk its way past, plus any
   ignore rules you add. The floor applies to the read tools and to the sandbox.
3. **OS sandbox.** For child processes only. Every `shell` command starts inside
   a sandbox; the alternative, `unsandboxed: true`, is a separate call that
   always asks.

The gates are independent on purpose: the sandbox bounds a process; it does not
replace the decision to run it. A command that leaves the default profile still
needs your approval, still clears the policy floor, and still runs under the
sandbox unless you approved `unsandboxed`.

## What prompts

| Action | Asks? |
|---|---|
| Read or search inside the workspace | no |
| Read or search a path the policy floor denies | yes |
| Edit or delete a path the session write rules allow | no |
| Edit or delete a path the session write rules deny | yes |
| `shell` in the default profile (sandboxed, no extra paths, network `deny`) | no |
| `shell` with `unsandboxed`, extra paths, or `network: "unrestricted"` | yes |
| `web_search` | always — each call spends quota |
| `web_fetch` to a host already used this session | no |
| `web_fetch` to a new host | yes |
| MCP tool, first call for that server + tool this session | yes |
| MCP tool already approved this session | no |
| `grant` | always |
| A subagent child call that would need approval | no prompt — it fails closed |

## The invariant

> User input never runs ahead of an unresolved turn.

If a turn paused for a decision, the next thing that happens settles it, whatever
you type. A paused turn is transcript state, so it survives a restart. You can
approve or reject each pending call; approving resumes the loop so the model
reads the result, and rejecting sends the model a structured rejection rather
than silence.

## Session grants

A `grant` appends one read or write allow for a path the session currently
refuses — including a path outside the workspace. The allow is **session-scoped**:

- it lives on that chat session and dies with it;
- a directory pattern matches the directory and its children;
- the workspace root is a broad allow that still loses to any literal deny, so it
  never opens `.env`, `.git`, or key material;
- a read grant is inherited read-only by a subagent, and never lets a child
  widen the parent's rules.

MCP approvals are a separate list on the session: the first call to a server's
tool asks, and "allow for this session" remembers that pair. See
[MCP](mcp.md) for the current state of that control.

## The policy floor

Built into the path filter and always applied, matched case-folded after symlink
resolution. Standard denies include:

- anything outside the workspace (`../`);
- `.git`;
- `.env` and `.env.*`;
- `*.pem` and `*.key`;
- `id_rsa`, `id_ed25519`;
- `credentials.json`, `secrets.json`.

The shell profile adds `~/.ssh`, `~/.aws`, `~/.kube`, `~/.gnupg`, the keychain,
and `~/.robi` to the denied set, and keeps the workspace's own `.git/config`,
`.git/hooks`, and the Robi config directory write-denied.

**Specificity decides a conflict.** The pattern whose match ends furthest into
the path wins; at the same point, the one with more literal characters wins; a
deny wins a remaining tie. So a wildcard allow like `src/.*` does not outrank a
literal `.env` deny.

## How it surfaces

A call that is pending approval renders as an approval bar: the verb and target,
then **Reject** and **Approve**. The full arguments are shown — a search shows
the query, a fetch shows the URL, an edit shows the diff — not the model's
summary. A call that ran without asking stays a result row. When the desktop
window is not in front, the pause raises an OS notification.

## Where this is specified

The three gates and the session-grant rules are in
[Read tools](../design/tools/read-tools.md); the sandbox, its profile, the
constructed `PATH`, and the network rules are in
[Shell tool](../design/tools/shell-tool.md). The approval lifecycle in the loop
is in [The agent loop](../design/core/agent-loop.md). A dedicated
`docs/src/design/workspace/permissions.md` page is queued; until it is written,
those three design docs are the source of truth.
