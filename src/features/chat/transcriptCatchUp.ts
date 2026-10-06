import type { AgentPhase } from '../../state/chatStore'

/** How long a busy phase may sit without a frame before the shell refetches. */
export const TRANSCRIPT_CATCH_UP_MS = 2000

/**
 * A turn that is still marked thinking or responding, and has not heard a
 * frame for this long, should reload the transcript. The row may already be
 * stored while the event that would paint it was dropped.
 */
export function transcriptCatchUpDue(phase: AgentPhase | undefined, quietMs: number): boolean {
  if (phase !== 'thinking' && phase !== 'responding') {
    return false
  }
  return quietMs >= TRANSCRIPT_CATCH_UP_MS
}
