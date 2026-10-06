# Frontend conventions

These rules apply when changing the Solid app in this directory. The desktop shell is Tauri. REST goes through the shared `openapi-fetch` client in `api/client.ts` (`paths` from `api/schema.d.ts`). Streaming uses one SSE connection. Visual tokens live in `docs/src/design/shell/visual-style.md`; do not invent one-off colors.

## Solid reactivity

Component functions run once. Read props and signals in JSX or in a tracking scope.

- Do not destructure props. Read `props.name`, or `splitProps` when passing rest props:

```typescript
const [local, rest] = splitProps(props, ["variant", "children"]);
```

- Do not write signals inside `createEffect`. Derive with `createMemo`:

```typescript
const doubleFoo = createMemo(() => foo() * 2);
```

- Do not `.map()` in JSX. Use `<For each={list()}>` for object arrays and `<Index each={list()}>` for primitives.
- Call signal getters: `count()`, not `count`.

## API

- Call the shared client. Do not add untyped `fetch` wrappers.
- Types come from the generated schema (`components["schemas"]["MyModel"]`). No `any` and no `as any`.
- Async data shown in the UI goes through `createResource` or a store in `state/`, with loading and error states explicit or bounded by `<Suspense>` / `<ErrorBoundary>`.

## SSE and streams

- Register `onCleanup` for every listener, socket, or stream (abort the controller).
- Patch incoming chunks into signals or a store with `produce` from `solid-js/store`. Do not replace a whole store object on every chunk.

## Tauri boundary

- HTTP and SSE carry product data.
- `@tauri-apps/api` is only for OS work: filesystem, tray, windows, notifications.
- Backend origin is `import.meta.env.VITE_API_BASE_URL` (empty means the Vite `/api` proxy). The desktop shell resolves the bound port in `api/client.ts`. Do not hardcode hosts or ports in components.

## Shape

The directory map is [frontend layout](../docs/src/design/shell/frontend-layout.md).

- Named component exports: `export function UserCard()`.
- Product UI goes in `features/<name>/`, split into small components in that directory.
- `components/layout/` and `components/ui/` stay generic. Do not add a feature screen there.
- API modules live in `api/`. Stores shared by more than one feature live in `state/`. A hook only one feature uses lives in that feature.
