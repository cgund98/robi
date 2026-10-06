/**
 * Route for the active chat. A real session has its own route so the window's
 * back and forward controls can walk between chats; the draft has none, so it
 * stays on the index route.
 */
export function chatRoute(activeSessionId: string | null, draftSelected: boolean): string {
  return !draftSelected && activeSessionId ? `/sessions/${activeSessionId}` : '/'
}
