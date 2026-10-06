# Frontend layout

The Solid app under `src/` is organized by feature. A feature owns the screens
and small components for one product area. Shared chrome and primitives stay
outside those slices so a feature can be split further without pulling shell
code with it.

## Layout

```
src/
  main.tsx                 # Solid entry
  app/                     # Shell: router host, window chrome, startup route
  api/                     # openapi-fetch client and generated types
  features/                # Vertical slices (chat, docs, review, settings, workspaces)
  components/
    ui/                    # Shared primitives (icons)
    layout/                # Window frame, sidebar, chat chrome
  state/                   # Stores read by more than one feature
  infra/                   # Tauri invoke boundary
  styles/                  # Global CSS and tokens
  mock/                    # Fixtures for tests
```

Add product UI under `src/features/<name>/`. Keep `components/` generic. A
screen that is one feature lives in that feature and is mounted from
`app/SolidApp.tsx`. Add `pages/` only when a route composes several features
and has no logic of its own.

## What goes where

| Path | Holds |
|------|--------|
| `features/<name>/` | Screens, the small components they are built from, and hooks only that feature uses |
| `components/layout/` | Chrome every screen shares: frame, sidebar, trays |
| `components/ui/` | Primitives with no product meaning |
| `app/` | The router host and hooks that belong to the window, not to one feature |
| `state/` | Client stores shared across features |
| `api/` | Typed HTTP. No feature UI |
| `infra/` | OS calls through `@tauri-apps/api` |

A hook that only one feature calls lives next to that feature
(`features/chat/useAgentEvents.ts`, `features/docs/useWorkspaceDocs.ts`). A
store that two features read stays in `state/`.

Split a screen into smaller components inside its feature directory. Do not
park those pieces in `components/` unless a second feature uses them with no
feature-specific behavior.

## Current features

| Feature | Role |
|---------|------|
| `chat` | Transcript, composer, tray, and the agent event hook |
| `docs` | Workspace documentation viewer |
| `review` | Session diff review |
| `settings` | Settings routes |
| `workspaces` | Workspace list |
