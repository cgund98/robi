import { sessionDisplayTitle, type ChatSession } from '../../api/sessions'

/**
 * A session row's activity mark.
 *
 * `running` wins over `awaiting`: a turn that is still executing is the thing
 * the user wants to see, and a session cannot be both at once in practice.
 */
export type SessionMark = 'running' | 'awaiting' | 'idle'

/** Rows the recents menu lists before it asks the user to filter instead. */
export const RECENTS_CAP = 20

type SessionRow = Pick<ChatSession, 'title' | 'has_pending_agent' | 'turn_display'>

export function sessionMark(session: SessionRow, phase: string | undefined): SessionMark {
  if (session.turn_display === 'awaiting_approval') {
    return 'awaiting'
  }
  if (
    session.has_pending_agent ||
    session.turn_display === 'pending' ||
    (phase !== undefined && phase !== 'idle')
  ) {
    return 'running'
  }
  return 'idle'
}

/** Case-insensitive match on the title the row shows. An empty query matches all. */
export function filterSessions<T extends SessionRow>(sessions: T[], query: string): T[] {
  const needle = query.trim().toLowerCase()
  if (!needle) {
    return sessions
  }
  return sessions.filter((session) => sessionDisplayTitle(session).toLowerCase().includes(needle))
}

/**
 * The filtered list cut to `RECENTS_CAP`, plus how many rows the cap hid.
 *
 * A menu is worse than a list once a user has more than a screenful, so the
 * hidden count is what tells the user to type in the filter field.
 */
export function visibleRecents<T extends SessionRow>(
  sessions: T[],
  query: string
): { rows: T[]; hidden: number } {
  const matched = filterSessions(sessions, query)
  return {
    rows: matched.slice(0, RECENTS_CAP),
    hidden: Math.max(0, matched.length - RECENTS_CAP)
  }
}
