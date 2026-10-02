# Visual style

Robi's desktop UI is a dark, two-column coding-agent shell. Color, density, and
layout follow Claude's desktop **Code** view; product behavior (tool cards,
approvals, modes) stays Robi's.

This page is the design doc for the **look**: tokens, shell layout, and the
visual treatment of chat chrome. Interaction details — streaming caret,
scroll-lock, tool-card expand defaults — live in
[chat-ui.md](chat-ui.md) (M2, still to write).

## What this doc does not cover

| Topic | Where it belongs |
|---|---|
| Streaming render, scroll-lock, message list mechanics, tool-card state | `docs/design/chat-ui.md` (M2) |
| IPC, event ordering, window lifecycle | `docs/design/architecture.md` (M2) |
| Approval policy and grant UX copy | `docs/design/permissions.md` (M3) |
| Mode names and transitions | `docs/design/agent-modes.md` (M5) |
| Full review window (diff views, hunk nav, inline comments) | `docs/design/code-review.md` (M6) |

## Problem

M2 ships a chat window before Robi has a visual identity. Without a written
palette and shell layout, every screen invents its own grays, and the UI drifts
toward either a generic dark dashboard or a light marketing shell.

Claude's desktop Code view already solves the composition we need: a narrow
session rail, a quiet transcript on near-black, a navy user bubble, warm muted
ink, and a bottom composer with model controls. Steal that surface language.
Do not steal Claude product features that Robi has not decided to ship.

## Decision

**Adopt Claude Code-view color and shell layout as Robi's default (and only)
theme for M2.** Dark-only. No light mode until product schedules one.

| Steal | Defer | Reject for now |
|---|---|---|
| Near-black canvas + charcoal sidebar/composer surfaces | Home / Code product toggle | Marketing light theme |
| Navy user message bubble | Routines / Scheduled lists | Card-heavy dashboard chrome |
| Warm off-white body ink (not pure white) | Voice / mic control | Purple accent stacks |
| Soft radii on bubbles, composer, sidebar pills | Pinned section as a first-class store | Floating badges over the transcript |
| Session rail + main transcript + bottom composer | Breadcrumb as editable project switcher | Multi-column agent canvases (Devin-style) |
| Muted activity lines between turns | Circular status ring as a meter | |
| **File-edit summary widget** with per-file `+/-` and a **Review ↗** link-out | Inline expanded diffs inside the transcript (prefer the review window) | Embedding a full PR review pane in the chat column |

Rejected alternatives:

- **Light-first / dual theme.** The reference is dark; a light theme doubles
  token work before any real chat UI exists.
- **Sync with Synchroni instrument tokens.** Different product; do not import
  hairline-rule / health-axis language here.
- **Terminal-green / CRT aesthetic.** Reads as novelty, not as a durable shell.

## Shell layout

One composition. Two columns. No third panel in M2.

```text
┌────────────┬──────────────────────────────────────────┐
│  Sidebar   │  Header (workspace / session title)      │
│            ├──────────────────────────────────────────┤
│  New       │                                          │
│  Sessions  │  Transcript                              │
│  Recents   │    user bubble                           │
│            │    assistant prose + inline code         │
│            │    muted activity lines                  │
│            │    (tool cards — see chat-ui)            │
│            │    (file-edit summary → Review ↗)        │
│            │                                          │
│            ├──────────────────────────────────────────┤
│  Settings  │  Composer + mode / model / effort row     │
└────────────┴──────────────────────────────────────────┘
```

| Region | Role | M2 content |
|---|---|---|
| **Sidebar** | Session navigation | New session, Recents list, active-session highlight, Settings link (no auth / profile) |
| **Header** | Orientation | Workspace name · session title (read-only crumb is fine for M2) |
| **Transcript** | The work | User bubbles, assistant text, activity lines, tool-call cards (empty until M3) |
| **Composer** | Primary input | Multiline field, send, mode, model, effort; stop when a turn is running |

Sidebar width stays roughly **240–280px** at default window size. The transcript
column takes the rest. The composer is anchored to the bottom of the main column,
not a floating overlay.

## Color tokens

Values sampled from Claude's desktop Code view and rounded to a small token set.
Implement as CSS custom properties on `:root` (and later a single theme file).
Do not sprinkle raw hex in components.

| Token | Value | Use |
|---|---|---|
| `--bg-canvas` | `#0b0b0b` | Main transcript background |
| `--bg-sidebar` | `#1f1f1e` | Sidebar fill |
| `--bg-surface` | `#1f1f1e` | Composer field, elevated chips |
| `--bg-surface-hover` | `#282827` | Row / control hover |
| `--bg-surface-active` | `#333333` | Selected session, pressed toggle |
| `--bg-user` | `#15202c` | User message bubble |
| `--ink` | `#c3c2b8` | Primary body text (warm, not pure white) |
| `--ink-strong` | `#ecece8` | Titles, emphasis |
| `--ink-muted` | `#8a8983` | Labels, timestamps, activity lines |
| `--ink-faint` | `#5c5c57` | Placeholders, disabled |
| `--accent` | `#55a2fb` | Focus ring, send/active status, links |
| `--accent-muted` | `#3a6fa8` | Accent on dark fills (icons in quiet states) |
| `--code-bg` | `#1a1a19` | Inline code chip background |
| `--code-ink` | `#d4d3cb` | Inline code text |
| `--rule` | `#2a2a28` | Hairline separators (use sparingly) |
| `--danger` | `#e56767` | Destructive / failed tool |
| `--success` | `#6fbf7a` | Completed tool / applied edit |
| `--diff-add` | `#5db27b` | `+N` line counts in edit summaries |
| `--diff-del` | `#c46b6b` | `-N` line counts in edit summaries |
| `--bg-row` | `#303030` | File rows inside the edit-summary widget |

`color-scheme: dark` on the document. Scrollbars and form controls should follow
the dark scheme.

### Accent discipline

`--accent` is for **state and focus**, not decoration. Do not tint large panels
blue. User messages stay navy (`--bg-user`); assistant messages stay unbubbled on
`--bg-canvas`.

## Typography

| Role | Stack | Notes |
|---|---|---|
| UI / body | `ui-sans-serif, system-ui, sans-serif` | Match the OS; no Inter/Roboto package |
| Code / paths | `ui-monospace, SFMono-Regular, Menlo, monospace` | Inline chips and fenced blocks |
| Body size | ~14–15px | Comfortable reading; avoid 12px transcript |
| Activity line | ~12–13px, `--ink-muted` | Quieter than assistant prose |
| Sidebar item | ~13–14px | Truncate with ellipsis; full title on hover |

Line height ~1.5 for assistant prose. Do not use a display serif or a marketing
font for the shell.

## Shape and spacing

| Token | Value | Use |
|---|---|---|
| `--radius-sm` | `6px` | Inline code chips, small buttons |
| `--radius-md` | `10px` | Session pills, toggles |
| `--radius-lg` | `14px` | User bubble, composer |
| `--radius-xl` | `16px` | Sidebar outer (if the shell floats the rail) |
| `--space-1` … `--space-6` | 4 / 8 / 12 / 16 / 24 / 32 px | Padding scale |

The reference uses **generous dark space**, not dense packing. Prefer padding
over borders. When a border is needed, use `--rule` at 1px — never a heavy
outline stack.

## Visual elements

These are the chrome pieces the style owns. Behavior of each is specified in
`chat-ui.md` when that page is written; this page locks how they look.

### User message

- Full-width bubble on `--bg-user` within the chat column, text `--ink`
  (or slightly cooler if needed for contrast on navy).
- Large radius (`--radius-lg`).
- The chat column itself caps at `--chat-column-width` (`48rem`); user bubbles,
  assistant prose, and the composer share that bound.

### Assistant message

- No bubble. Prose sits on `--bg-canvas` in `--ink`.
- Inline code: `--code-bg` chip, `--radius-sm`, mono stack.
- Fenced code blocks: same surface family, slightly taller padding; syntax
  highlighting comes later and must stay readable on `#0b0b0b`.

### Activity lines

Muted one-liners between turns (e.g. “Read 3 files…”). Icon + `--ink-muted`
text. Not cards. Tool **results** that need inspection become tool cards
(`chat-ui.md`); activity lines are the collapsed summary strip.

### Session rail items

- Idle: transparent / canvas-adjacent.
- Hover: `--bg-surface-hover`.
- Active: `--bg-surface-active` pill with `--radius-md`.
- Section labels (Recents): uppercase or small caps optional; prefer plain muted
  label text over heavy chrome.

### Composer

- Tall rounded field on `--bg-surface`, placeholder `--ink-faint`.
- Send control on the right inside the field (arrow / return affordance).
- Below the field: right cluster (model, effort, run status). Controls are quiet
  text + chevron, not colored pills — except the active run indicator, which may
  use `--accent`. No mode toggle or attach control in the shell mock.

### Header

Single quiet line: `workspace / session title`. No toolbar of icons in M2.
Chevron for a future session switcher is allowed as a disabled or no-op affordance
only if it does not imply unfinished product.

### File-edit summary widget

A compact block that appears in the transcript after the agent has edited files.
It is the **bridge from chat to review**: skim here, open the full review
window from the link. Reference: Claude's "N files edited" strip.

```text
  2 files edited   +123  -42                    Review ↗
  ┌─────────────────────────────────────────────────────┐
  │  slider.tsx              +83  -0   ●            ▾  │
  ├─────────────────────────────────────────────────────┤
  │  page.tsx                +40  -42               ▾  │
  ├─────────────────────────────────────────────────────┤
  │  background.tsx          +15  -0                ▾  │
  └─────────────────────────────────────────────────────┘
```

| Piece | Look |
|---|---|
| **Header left** | Muted summary (`N files edited`) + aggregate `+` / `-` in `--diff-add` / `--diff-del` |
| **Header right** | `Review ↗` in `--ink-muted`; hover → `--ink`. Opens the review window (M6), not an in-chat diff pane |
| **File rows** | `--bg-row`, `--radius-md`, filename in UI sans (or mono if paths get long), per-file `+/-`, chevron on the right |
| **Unread / focus dot** | Optional `--accent` disc on a row that has not been opened in review yet |
| **Density** | Tight vertical stack; rows are chips, not large cards |

**Milestone split**

| When | What ships |
|---|---|
| **M4** (editing) | Widget appears after successful edits; rows list paths and line counts; chevron may expand a short preview or stay collapsed |
| **M6** (code review) | `Review ↗` is live and opens the review session for this turn's edits |

Until M6, show the header without a dead link — either omit `Review ↗` or render it disabled with no fake destination.

Do **not** paste unified diffs into the transcript by default. The widget stays a
summary; the review window owns hunks, side-by-side, and comments
(`code-review.md`).

## CSS architecture (when implementing)

- Put tokens in `src/styles/tokens.css` (or `:root` in `global.css` until a
  split is needed).
- One CSS Module per component / region (`Sidebar`, `Transcript`, `Composer`).
- Do not add Tailwind or a component kit. Tokens + modules only.
- Delete a module when its component goes away.

## Open decisions

| # | Decision | Notes |
|---|---|---|
| V1 | Sidebar flush to the window edge vs. inset floating rail | **Chosen: inset floating rail** with `--radius-xl` on a canvas gutter |
| V2 | Exact sidebar width and whether it is resizable | Start fixed (~260px); resize is polish |
| V3 | Whether assistant ever gets a bubble | Default no; revisit only if contrast testing fails |
| V4 | Brand mark in the sidebar | None in M2 unless a simple wordmark is ready |
| V5 | File-row chevron: expand inline snippet vs. jump to that file in review | Prefer jump-to-review once M6 exists; until then a collapsed-only row is fine |
| V6 | Aggregate counts: lines changed vs. files touched | Reference uses line counts (`+123 -42`); keep that unless editing tools report only file-level stats |

## Failure modes

- **Pure white body text** on `#0b0b0b` will look harsher than the reference;
  stick to `--ink`.
- **Blue-tinted panels** break the quiet canvas; keep `--accent` for controls.
- **Light mode sneak-in** via `color-scheme: light` or system form defaults —
  set dark explicitly.
- **Porting Synchroni / shadcn patterns** will fight this palette; do not.

## Relationship to later docs

When `chat-ui.md` is written, it should assume these tokens and regions, and
place the file-edit summary in the message list. When `code-review.md` is
written, it owns the destination of `Review ↗`. When a component needs a new
color, add a token here first — do not invent a one-off hex in the module.
