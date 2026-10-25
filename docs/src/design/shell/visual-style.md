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
| Streaming render, scroll-lock, message list mechanics, tool-card state | `docs/src/design/shell/chat-ui.md` (M2) |
| IPC, event ordering, window lifecycle | `docs/src/design/core/architecture.md` (M2) |
| Approval policy and grant UX copy | `docs/src/design/workspace/permissions.md` (M3) |
| Mode names and transitions | `docs/src/design/core/agent-modes.md` (M5) |
| Full review window (diff views, hunk nav, inline comments) | `docs/src/design/review/code-review.md` (M6) |

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

Rejected alternatives:

- **Light-first / dual theme.** The reference is dark; a light theme doubles
  token work before any real chat UI exists.
- **Sync with Synchroni instrument tokens.** Different product; do not import
  hairline-rule / health-axis language here.
- **Terminal-green / CRT aesthetic.** Reads as novelty, not as a durable shell.

## Shell layout

One composition: a single bar, then the transcript across the whole window.

```text
┌──────────────────────────────────────────────────────────┐
│  ‹ ›   Workspace ▾       Session title ▾       ＋  ⚙  ⋯   │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  Transcript                                              │
│    user bubble                                           │
│    assistant prose + inline code                         │
│    muted activity lines                                  │
│    (tool cards — see chat-ui)                            │
│                                                          │
├──────────────────────────────────────────────────────────┤
│  Composer + mode / model / effort row                    │
└──────────────────────────────────────────────────────────┘
```

The top row is the window bar. It is the same height on every page, and it is the
drag region. On the chat shell the whole row is `--bg-canvas`; on Settings the
split follows the settings rail; on Workspaces the whole row is `--bg-canvas`.
macOS window buttons sit in that row, inset from the window corner, so the bar's
leading edge starts after them: `--traffic-light-inset` is `84px` on macOS and
`0px` elsewhere. The desktop window uses an overlay title bar so those buttons
draw on the app fill instead of a separate system bar.

There is no permanent left column (streamlining S6). The legacy two-column
layout — a `--bg-sidebar` column with the chat header in that same row — is still
in the tree behind `showSurface('topBarShell')` in `src/app/surfaces.ts`, and the
references to `--sidebar-width` in this page describe it.

| Region | Role | Content |
|---|---|---|
| **Window bar** | Drag region and navigation | macOS window controls, inset from the corner. Then the workspace dropdown, back and forward, and the session title — which is also the Recents menu. At the trailing edge: New chat, Settings, and an overflow holding MCP marks and the API health row |
| **Transcript** | The work | User bubbles, assistant text, activity lines, tool-call cards |
| **Composer** | Primary input | Multiline field, send, mode, model, effort; stop when a turn is running |

Nothing but the text column is capped. After the first message, the composer stays at the
bottom of the main column and the transcript scrolls underneath it. The
scrollbar runs to the bottom of that column. A scrim over the bottom of the
transcript fades into `--bg-canvas` and is solid halfway down the composer,
so the rows behind the lower half are gone. The fade reaches `1rem` above
the dock. Padding under the last row is the dock height plus `--space-4`, so
the last row sits just above the composer when the list is scrolled to the end. Before the first
message, the composer sits under the greeting in the center of the column.

### Bar rules

- **One row at the default width.** The title truncates before any control does.
  The primary actions (workspace, title/Recents, New chat, Settings) stay visible;
  MCP marks and the API health row fold into one overflow menu, and that button is
  absent while there is nothing to show.
- **The workspace name collapses below 1040 px.** At narrow widths the dropdown
  keeps its folder icon and chevron and drops the name, before anything clips.
- **The overflow is the only home for secondary chrome.** It is a Kobalte
  dropdown, styled with these tokens.

## Color tokens

Values sampled from Claude's desktop Code view and rounded to a small token set.
Implement as CSS custom properties on `:root` (and later a single theme file).
Do not sprinkle raw hex in components.

| Token | Value | Use |
|---|---|---|
| `--bg-canvas` | `#0b0b0b` | Main transcript background |
| `--bg-sidebar` | `#1f1f1e` | Sidebar fill. The legacy layout only |
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
| `--warn` | `#e8c547` | Yellow text for the review **Reject with reason** menu item and its **Reject and send** button |
| `--mode-ask` | `#6fbf7a` | Ask mode in the composer |
| `--mode-plan` | `#e39a3c` | Plan mode, and the plan Build and View Plan buttons |
| `--attention` | `#e39a3c` | A tool call waiting on a decision: the card's left rail. Same value as `--mode-plan`, named separately so "this needs you" can diverge from "this session is in plan mode" |
| `--diff-add` | `#5db27b` | `+N` line counts in edit summaries |
| `--diff-del` | `#c46b6b` | `-N` line counts in edit summaries |
| `--find-hit` | `#6b5416` | Find-in-document match background |
| `--find-hit-current` | `#e39a3c` | Active find-in-document match |
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
- The chat column itself caps at `--chat-column-width` (`48rem`) and stays centered in the main pane when the window is wider. User bubbles, assistant prose, and the composer share that bound.

### Assistant message

- No bubble. Prose sits on `--bg-canvas` in `--ink-bright`.
- Inline code: `--code-bg` chip, `--radius-sm`, mono stack.
- Fenced code blocks: same surface family, slightly taller padding; syntax
  highlighting comes later and must stay readable on `#0b0b0b`.

### Mermaid diagrams

- A fenced `mermaid` block renders on `--bg-canvas`, the same ground as the
  transcript, with `--radius-md` and `--space-3` padding. The diagram sits
  centered in it. The ground is the SVG's own `background` /
  `edgeLabelBackground` value, so the diagram's `--bg-surface` nodes and
  `--code-bg` containers step up from it. On `--code-bg` the containers were the
  same color as the block and the nodes barely differed, so the diagram read as
  a flat patch.
- The surface scrolls horizontally and caps at `max-width: 100%`, so a diagram
  wider than the chat column scrolls like a wide code block instead of
  stretching the column.
- The SVG uses the same tokens as the shell, not mermaid's default palette:
  node fill `--bg-surface`, borders and edges `--rule`, node text
  `--ink-bright`, container fill `--code-bg`, on the `--bg-canvas` background.
  Sequence diagrams use the same tokens: actor boxes `--bg-surface` on `--rule`
  borders, lifelines and messages `--rule` and `--ink`, notes on `--code-bg`.
  State composites (a state that contains states) fill with `--code-bg` on both
  the title and the body. The diagram uses mermaid's classic look, which is a
  flat fill. Mermaid's default neo look adds a light gray drop shadow, and on
  this ground that shadow reads as a second fill where a box sits inside a box.
- Diagram text is the UI sans stack at 14px, a little smaller than body copy,
  matching the fenced code block it replaces.
- While the render is in flight, and when mermaid cannot parse the source, the
  block is an ordinary fenced code block, not a placeholder surface. Mermaid's
  error diagram is suppressed; the failure is logged.

### Activity lines

Muted one-liners between turns (e.g. “Read 3 files…”). Icon + `--ink-muted`
text. Not cards. Tool **results** that need inspection become tool cards
(`chat-ui.md`); activity lines are the collapsed summary strip.

### Recents menu

The session title in the window bar is the trigger: a quiet line with a chevron,
`--ink` at 13px, a `--bg-surface-hover` fill on hover and while open, and
`--ink-strong` then. Its accessible name is `<title> — recents`.

- The panel is `--bg-canvas` on a 1px `--ink-faint` edge, `--radius-md`, and the
  same shadow the workspace menu uses. It is a menu, not a panel: no header bar,
  no footer.
- First row is the **filter field**, `--bg-surface-hover` with a
  `--bg-surface-active` border, and focus moves that border to `--ink-faint`. It
  takes focus when the menu opens, and the query clears when the menu closes.
- Rows are 13px, `--bg-surface-hover` under the pointer or the keyboard, and
  `--bg-surface-active` for the open session. The title truncates with an
  ellipsis and carries the full text on hover.
- Marks sit at the start of a row and match the transcript's: a running agent is
  a 12px spinning ring, `--ink-faint` with an `--ink` leading edge; an approval
  is an 8px `--accent` dot. Reduced motion leaves the ring still.
- **Rename** (✎) and **Delete** (×) appear at the trailing edge of a row on hover
  or focus, and open the same dialogs the sidebar used. Their space is reserved
  whether or not they show, so revealing them never changes a row's height or
  shifts the list.
- The list caps at 20 rows. A line under it says how many are hidden and asks the
  user to type: `N more sessions — type to narrow the list.`

### Composer

- Tall rounded field on `--bg-surface`, placeholder `--ink-faint`.
- Send control on the right inside the field (arrow / return affordance). While a turn is running, that control is a filled circle with a rounded square cut out of it, in the same slot.
- Below the field: mode on the left, and model, effort, and the context meter on the right. Controls are quiet
  text + a Lucide `ChevronDown`, not colored pills. The meter is a 14px ring. The track is
  `--ink-faint` and the filled share is `--ink-muted`. Clicking it opens a
  popover. No attach control in the shell mock.
- An empty chat lifts that same field into one bordered card under the greeting.
  The model row sits inside the card, without the context meter. The greeting is
  the workspaces title type, with no mark beside it.

### Settings

A full-page shell, not a dialog. Left rail: back to the chat, then the section
list. Right pane: a title, then one or more sections, in the same centered
column as the chat (`--chat-column-width`). Each section is a muted
heading and a bordered card of rows. **General** starts with **Display**: one row
with the UI scale stepper (−, the percentage, +, and Reset) and the Cmd/Ctrl `+`,
`-`, and `0` shortcuts beside it. The level scales the whole window and is
remembered per device. Then **Model Defaults**
(one row per Global, Agent, Ask, and Plan: mode name, model menu on the left,
effort menu on the same row). An unset mode shows Global Default
in those menus, and that item clears the override. Global effort stays a
segmented control. **OpenCode Go** and **Anthropic** are each their own card:
an enabled switch, on by default, then that provider's API key. OpenCode Go also
has the optional base URL. A switch that is off disables that card's key field. **Permissions** is a single card: switches for
web search approval and web fetch approval, both on by default. Below model
defaults, **General** has the workspace, **Files** (newline-separated globs for paths that open outside the viewer, and an optional command such as `zed {file_path}:{line}`; an empty command uses the system app; a command that fails shows its error on the edit card; everything else opens in the file viewer), a language-server switch, then max
iterations and subagent max iterations, a subagent timeout, and a tool timeout.
**Audit log**
lists the last 250 API calls in a table (time, method, path, status, duration), 50 per page. Clicking a row shows that response body. Errors from this run of the app follow, newest first. Health checks are omitted. It is not saved. Use the same tokens;
active nav is `--bg-surface-active`, the selected effort pill may use `--accent`.
Text fields, including secrets, sit one step above the card (`--bg-surface-hover`)
with a `--bg-surface-active` border. Focus moves that border to `--ink-faint`.
They are not canvas wells.

### Header

One bar holds the navigation. From the leading edge: back and forward, then the
workspace dropdown, then the session title centred over the transcript — and that
title is the Recents menu's trigger. Back and forward walk the same history as
the side mouse buttons: each chat and the draft. A button is quiet when that
direction has no entry. At the trailing edge sit New chat (`SquarePen`) and
Settings, then the overflow if anything lives in it: one row per MCP
server, and a row for a failing API health check. The index pill used to sit
here while the workspace index was not ready; S3 removed it with the index.
The transcript starts on the next row. Every icon-only control carries an
accessible name and a title, and tab order runs leading, centre, trailing. The
first time a session is opened, the main column shows a spinner and “Loading
conversation” until that transcript arrives. Opening it again uses the copy
already in memory and does not show that spinner.

The workspace dropdown sits after the history pair. The workspace menu is a
Kobalte dropdown, and the rename and delete
dialogs are Kobalte dialogs. Both are styled with these tokens. The active
workspace uses an open folder. The others use a closed folder. The closed control
is an inline chip — icon, name, chevron — with no border until hover. The open
list is the darker canvas color, with a stronger edge. The chevron opens the list
of workspaces, most recently selected first. The menu shows at most
eight workspaces, and one **Manage workspaces…** item under a rule links to the
Workspaces page. A workspace with no recorded selection stays in the order the
API returned it. Adding a workspace happens on that page, not in this menu.

### Overflow menu

One row per MCP server: the server's icon, then `name · status · N tools` on one
line, where the status is `connected`, `starting`, `failed`, or `disconnected`
and the tool count appears only when connected. Each row links to MCP settings.
A failing API health check adds a line under them in `--danger`. The rows match
the **Servers** list on the MCP settings page, in the same order.

### Workspaces page

A full-page list at `/workspaces`, in the same role as a projects home. The
page content sits in the same centered column as the chat (`--chat-column-width`). The
title sits on the left of that column. Search and **New workspace** sit on the right. With no workspaces, the column center holds a short prompt
and the same create action. With some, the cards stack in that column, each as
wide as the column. The name and Open / Remove share the top line. The path sits
below on a darker strip in mono. Opening a card selects it and returns to the
chat. The chat shell sends you here when the list is empty. **Chat** returns
when one is already selected.

### Edited-file strip — removed

The line above the composer that read `N files edited` with a **Review** button is
gone with the review screen (streamlining S2). The implementation
(`src/features/chat/EditReviewStrip.tsx`) stays behind
`showSurface('reviewScreen')`. Reviewing a change means `git diff`
([Reviewing changes with git](../../guides/code-review.md)).

Do not paste diffs into the transcript instead. The inline file peek from a tool
card is the one in-app diff, and it shows a single file on demand.

## Icons

Interface icons come from [Lucide](https://lucide.dev) (`lucide-react`). Pick an existing Lucide icon instead of drawing a new SVG. Size and stroke stay with the control: sidebar nav is 18px at stroke 1.75, and smaller chrome is 12–14px. Color is `currentColor`, so the icon inherits the text token. The context meter is a usage ring, and Mermaid output is a diagram. Neither is an icon.

## CSS architecture (when implementing)

- Put tokens in `src/styles/tokens.css` (or `:root` in `global.css` until a
  split is needed).
- One CSS Module per component / region (`Sidebar`, `Transcript`, `Composer`).
- Do not add Tailwind or a component kit. Tokens + modules only. Icons are Lucide, not a second icon set.
- Delete a module when its component goes away.

## Open decisions

| # | Decision | Notes |
|---|---|---|
| V1 | Sidebar flush to the window edge vs. inset floating rail | Moot: there is no sidebar. The bar spans the window |
| V2 | Sidebar width and whether it is resizable | Only the legacy layout has one (~260px, `--sidebar-width`) |
| V3 | Whether assistant ever gets a bubble | Default no; revisit only if contrast testing fails |
| V4 | Brand mark in the sidebar | There is no sidebar and no mark; the workspace dropdown leads the bar |
| V5 | File-row chevron: expand inline snippet vs. jump to that file in review | The inline file peek is the destination. No surface lists edited files |
| V6 | Aggregate counts: lines changed vs. files touched | Reference uses line counts (`+123 -42`); keep that unless editing tools report only file-level stats |

## Failure modes

- **Pure white body text** on `#0b0b0b` will look harsher than the reference;
  stick to `--ink`.
- **Blue-tinted panels** break the quiet canvas; keep `--accent` for controls.
- **Light mode sneak-in** via `color-scheme: light` or system form defaults —
  set dark explicitly.
- **Porting Synchroni / shadcn patterns** will fight this palette; do not.

## Relationship to later docs

`chat-ui.md` assumes these tokens and regions. [code-review.md](../review/code-review.md)
describes the removed review screen. When a component needs a new color, add a
token here first — do not invent a one-off hex in the module. Syntax colors on a
diff line come from the highlighter theme, not from a new token.
