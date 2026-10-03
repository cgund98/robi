# Visual style

Robi's desktop UI is a dark, two-column coding-agent shell. Color, density, and
layout follow Claude's desktop **Code** view; product behavior (tool cards,
approvals, modes) stays Robi's.

This page is the design doc for the **look**: tokens, shell layout, and the
visual treatment of chat chrome. Interaction details — the draft session,
activity line, and composer lock — live in [chat-ui.md](chat-ui.md). Streaming
caret and tool-card expand defaults are still open there.

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
session rail, a quiet transcript on near-black, a user bubble slightly
lighter than the canvas, warm muted
ink, and a bottom composer with model controls. Steal that surface language.
Do not steal Claude product features that Robi has not decided to ship.

## Decision

**Adopt Claude Code-view color and shell layout as Robi's default (and only)
theme for M2.** Dark-only. No light mode until product schedules one.

| Steal | Defer | Reject for now |
|---|---|---|
| Near-black canvas + charcoal sidebar/composer surfaces | Home / Code product toggle | Marketing light theme |
| User bubble slightly lighter than the canvas | Routines / Scheduled lists | Card-heavy dashboard chrome |
| Warm off-white body ink (not pure white) | Voice / mic control | Purple accent stacks |
| Soft radii on bubbles, composer, sidebar pills | Pinned section as a first-class store | Floating badges over the transcript |
| Session rail + main transcript + bottom composer | Breadcrumb as editable project switcher | Multi-column agent canvases (Devin-style) |
| Muted activity lines between turns, and a context ring as a meter | | |
| **Edited-file strip** above the composer, with a count and **Review** | Inline expanded diffs inside the transcript | Embedding a full PR review pane in the chat column |

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
│  Workspace │  Header (session title)                  │
│            ├──────────────────────────────────────────┤
│  New       │                                          │
│  Recents   │  Transcript                              │
│            │    user bubble                           │
│            │    assistant prose + inline code         │
│            │    muted activity lines                  │
│            │    (tool cards — see chat-ui)            │
│            │                                          │
│            ├──────────────────────────────────────────┤
│  Settings  │  N files edited              Review      │
│            │  Composer + mode / model / effort row     │
└────────────┴──────────────────────────────────────────┘
```

| Region | Role | M2 content |
|---|---|---|
| **Sidebar** | Session navigation | Workspace dropdown at the top, Workspaces link under it, New chat, Recents list, active-session highlight, rename dialog, Settings link |
| **Header** | Orientation | Session title |
| **Transcript** | The work | User bubbles, assistant text, activity lines, tool-call cards (empty until M3) |
| **Composer** | Primary input | Multiline field, send, mode, model, effort; stop when a turn is running |

Sidebar width stays roughly **240–280px** at default window size. The transcript
column takes the rest. After the first message, the composer stays at the
bottom of the main column and the transcript scrolls underneath it. The
scrollbar runs to the bottom of that column. A scrim over the bottom of the
transcript fades into `--bg-canvas` and is solid halfway down the composer,
so the rows behind the lower half are gone. The fade reaches `1rem` above
the dock. Padding under the last row is the dock height plus `--space-4`, so
the last row sits just above the composer when the list is scrolled to the end. Before the first
message, the composer sits under the greeting in the center of the column.

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
| `--bg-user` | `#181817` | User message bubble, a dark gray lighter than `--bg-canvas` |
| `--ink` | `#c3c2b8` | Primary body text (warm, not pure white) |
| `--ink-strong` | `#ecece8` | Titles, emphasis |
| `--ink-bright` | `#f7f6f2` | Assistant prose |
| `--ink-muted` | `#8a8983` | Labels, timestamps, activity lines |
| `--ink-faint` | `#5c5c57` | Placeholders, disabled |
| `--accent` | `#55a2fb` | Focus ring, send/active status, links |
| `--accent-muted` | `#3a6fa8` | Accent on dark fills (icons in quiet states) |
| `--code-bg` | `#1a1a19` | Inline code chip background |
| `--code-ink` | `#d4d3cb` | Inline code text |
| `--rule` | `#2a2a28` | Hairline separators (use sparingly) |
| `--danger` | `#e56767` | Destructive / failed tool |
| `--success` | `#6fbf7a` | Completed tool / applied edit |
| `--mode-ask` | `#6fbf7a` | Ask mode in the composer |
| `--mode-plan` | `#e39a3c` | Plan mode, and the plan Build and View Plan buttons |
| `--diff-add` | `#5db27b` | `+N` line counts in edit summaries |
| `--diff-del` | `#c46b6b` | `-N` line counts in edit summaries |
| `--bg-row` | `#303030` | File rows inside the edit-summary widget |

`color-scheme: dark` on the document. Form controls follow the dark scheme.
Scrollbar thumbs are `--bg-surface-active`, thin, with a transparent track.

### Accent discipline

`--accent` is for **state and focus**, not decoration. Do not tint large panels
blue. User messages stay a dark gray (`--bg-user`); assistant messages stay unbubbled on
`--bg-canvas`.

## Typography

| Role | Stack | Notes |
|---|---|---|
| UI / body | `ui-sans-serif, system-ui, sans-serif` | Match the OS; no Inter/Roboto package |
| Code / paths | `ui-monospace, SFMono-Regular, Menlo, monospace` | Inline chips and fenced blocks |
| Body size | ~14–15px | Comfortable reading; avoid 12px transcript |
| Activity line | ~12–13px, `--ink-muted` | Quieter than assistant prose |
| Sidebar item | ~13–14px | Truncate with ellipsis; full title on hover |
| Empty greeting | ~32px, UI sans, weight 560 | Same type as the workspaces page title |

Line height ~1.5 for assistant prose. The empty-chat greeting uses the same
sans title as the workspaces page. The rest of the shell stays on the UI sans.

## Shape and spacing

| Token | Value | Use |
|---|---|---|
| `--radius-sm` | `6px` | Inline code chips, small buttons |
| `--radius-md` | `10px` | Session pills, toggles |
| `--radius-lg` | `14px` | User bubble, composer |
| `--radius-xl` | `16px` | Large floating surfaces. The sidebar does not use it |
| `--space-1` … `--space-6` | 4 / 8 / 12 / 16 / 24 / 32 px | Padding scale |

The reference uses **generous dark space**, not dense packing. Prefer padding
over borders. When a border is needed, use `--rule` at 1px — never a heavy
outline stack.

## Visual elements

These are the chrome pieces the style owns. Behavior of each is specified in
`chat-ui.md`; this page locks how they look.

### User message

- Bubble on `--bg-user`, aligned to the chat column's right edge. It is only
  as wide as its text, and never wider than 85% of the column. Text is
  `--ink-bright` so it matches the brightness of assistant prose.
- Large radius (`--radius-lg`).
- The chat column itself caps at `--chat-column-width` (`48rem`); user bubbles,
  assistant prose, and the composer share that bound.

### Assistant message

- No bubble. Prose sits on `--bg-canvas` in `--ink-bright`.
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
- A running agent: a 12px ring at the start of the row, `--ink-faint` with an
  `--ink` leading edge, spinning. Grayscale, not `--accent`. Reduced motion
  leaves the ring still.
- Section labels (Recents): uppercase or small caps optional; prefer plain muted
  label text over heavy chrome.

### Composer

- Tall rounded field on `--bg-surface`, placeholder `--ink-faint`.
- Send control on the right inside the field (arrow / return affordance). While a turn is running, that control is a stop square in the same slot.
- Below the field: mode on the left, and model, effort, and the context meter on the right. Controls are quiet
  text + chevron, not colored pills. The meter is a 14px ring. The track is
  `--ink-faint` and the filled share is `--ink-muted`. Clicking it opens a
  popover. No attach control in the shell mock.
- An empty chat lifts that same field into one bordered card under the greeting.
  The model row sits inside the card. The greeting is the workspaces title
  type, with no mark beside it.

### Settings

A full-page shell, not a dialog. Left rail: back to the chat, then the section
list. Right pane: a title, then one or more sections. Each section is a muted
heading and a bordered card of rows. **Model Providers** has **Model Defaults**
(one row per Global, Agent, Ask, and Plan: mode name, model menu on the left,
effort menu on the same row). An unset mode shows the Global model and effort
in those menus, and its Default item clears the override. Global effort stays a
segmented control. and a separate card per provider. OpenCode
holds the API key and base URL. **General** is a single card. Use the same tokens;
active nav is `--bg-surface-active`, the selected effort pill may use `--accent`.
Text fields, including secrets, sit one step above the card (`--bg-surface-hover`)
with a `--bg-surface-active` border. Focus moves that border to `--ink-faint`.
They are not canvas wells.

### Header

The session title is a single quiet line. No toolbar of icons.

The workspace dropdown is the top of the sidebar, in place of a product title.
The workspace menu is a Radix dropdown, and the rename dialog is a Radix dialog.
Both are styled with these tokens. The active workspace uses an open folder. The others use a closed folder. The closed control matches the sidebar, with a
chevron. A border appears on hover and while the list is open. The open list is the darker canvas color, with a stronger
edge. The chevron opens the list of workspaces, plus add. Add opens the system folder dialog in the
desktop window, and asks for a path in a normal browser. **Workspaces**, **New chat**, and **Settings** share one nav style: 14px `--ink-strong`, with an 18px icon in the same color. New chat uses the compose mark, a rounded square with a pencil. Workspaces sits under the dropdown and opens the full list.

### Workspaces page

A full-page list at `/workspaces`, in the same role as a projects home. The
title sits on the left. Search and **New workspace** sit on the right. With no workspaces, the canvas center holds a short prompt
and the same create action. With some, the cards stack in one column, each as
wide as the page. The name and Open / Remove share the top line. The path sits
below on a darker strip in mono. Opening a card selects it and returns to the
chat. The chat shell sends you here when the list is empty. **Chat** returns
when one is already selected.

### Edited-file strip

A single line fixed above the composer, in the same column width. It appears
when this session has at least one path whose baseline differs from the file
on disk. It does not scroll with the transcript, and it is absent on an empty
chat.

```text
      ┌───────────────────────────────────────────┐
      │  2 files edited                [ Review ] │
  ╭───┴───────────────────────────────────────────┴───╮
  │  Describe a task…                                 │
  ╰───────────────────────────────────────────────────╯
```

| Piece | Look |
|---|---|
| **Bar** | `--bg-canvas` fill, the same ground as the transcript, with a `1px` `--rule` border. Top corners `--radius-lg`, square bottom. It is inset by `--radius-lg` on each side so its edges meet the composer where the composer’s top curve ends |
| **Left** | `N file(s) edited` in `--ink` |
| **Right** | `Review` on `--bg-surface-active`, `--ink-strong`, `--radius-sm`. Hover lightens the fill. Opens `#/sessions/:id/review` |

Do not paste diffs into the transcript. The review screen owns the tree and
the hunks ([code-review.md](code-review.md)).

## CSS architecture (when implementing)

- Put tokens in `src/styles/tokens.css` (or `:root` in `global.css` until a
  split is needed).
- One CSS Module per component / region (`Sidebar`, `Transcript`, `Composer`).
- Do not add Tailwind or a component kit. Tokens + modules only.
- Delete a module when its component goes away.

## Open decisions

| # | Decision | Notes |
|---|---|---|
| V1 | Sidebar flush to the window edge vs. inset floating rail | **Chosen: flush.** The rail meets the top, left, and bottom of the window. Square corners, no canvas gutter |
| V2 | Exact sidebar width and whether it is resizable | Start fixed (~260px); resize is polish |
| V3 | Whether assistant ever gets a bubble | Default no; revisit only if contrast testing fails |
| V4 | Brand mark in the sidebar | The workspace dropdown occupies the top of the sidebar |
| V5 | File-row chevron: expand inline snippet vs. jump to that file in review | The review screen is the destination. The chat strip does not list files |
| V6 | Aggregate counts: lines changed vs. files touched | Reference uses line counts (`+123 -42`); keep that unless editing tools report only file-level stats |

## Failure modes

- **Pure white body text** on `#0b0b0b` will look harsher than the reference;
  stick to `--ink`.
- **Blue-tinted panels** break the quiet canvas; keep `--accent` for controls.
- **Light mode sneak-in** via `color-scheme: light` or system form defaults —
  set dark explicitly.
- **Porting Synchroni / shadcn patterns** will fight this palette; do not.

## Relationship to later docs

`chat-ui.md` assumes these tokens and regions. The edited-file strip sits
above the composer. [code-review.md](code-review.md) owns the review screen.
When a component needs a new color, add a token here first — do not invent a
one-off hex in the module. Syntax colors on a diff line come from the
highlighter theme, not from a new token.
