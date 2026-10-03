import type { ReviewHunk, ReviewLine } from '../../api/review'

export type ReviewView = 'diff' | 'current' | 'previous'

export function linesForView(lines: ReviewLine[], view: ReviewView): ReviewLine[] {
  const kept = lines.filter((line) => {
    if (view === 'current' && line.kind === 'delete') {
      return false
    }
    if (view === 'previous' && line.kind === 'insert') {
      return false
    }
    return true
  })
  return trimGaps(kept)
}

function trimGaps(lines: ReviewLine[]): ReviewLine[] {
  const out: ReviewLine[] = []
  for (const line of lines) {
    if (line.kind === 'gap') {
      if (out.length === 0 || out[out.length - 1].kind === 'gap') {
        continue
      }
    }
    out.push(line)
  }
  while (out.length > 0 && out[out.length - 1].kind === 'gap') {
    out.pop()
  }
  return out
}

export type ReviewChunk = {
  hunkIds: string[]
  lines: ReviewLine[]
}

/** Visual hunks, split on gaps. Each one carries the hunk ids it can approve or reject. */
export function chunksFor(lines: ReviewLine[], hunks: ReviewHunk[]): ReviewChunk[] {
  const groups: ReviewLine[][] = []
  let current: ReviewLine[] = []
  for (const line of lines) {
    if (line.kind === 'gap') {
      if (current.length > 0) {
        groups.push(current)
        current = []
      }
      continue
    }
    current.push(line)
  }
  if (current.length > 0) {
    groups.push(current)
  }
  return groups.map((group) => ({
    lines: group,
    hunkIds: hunks.filter((hunk) => hunkTouches(hunk, group)).map((hunk) => hunk.id)
  }))
}

function hunkTouches(hunk: ReviewHunk, lines: ReviewLine[]): boolean {
  return lines.some((line) => {
    if (line.kind === 'delete' && line.old_line != null && hunk.old_count > 0) {
      return line.old_line > hunk.old_start && line.old_line <= hunk.old_start + hunk.old_count
    }
    if (line.kind === 'insert' && line.new_line != null && hunk.new_count > 0) {
      return line.new_line > hunk.new_start && line.new_line <= hunk.new_start + hunk.new_count
    }
    return false
  })
}

/** A one-line note when hiding a side leaves an added or deleted file empty. */
export function reviewNote(
  status: 'added' | 'deleted' | 'modified',
  view: ReviewView,
  lines: ReviewLine[]
): string | null {
  if (lines.some((line) => line.kind !== 'gap')) {
    return null
  }
  if (status === 'added' && view === 'previous') {
    return 'File added'
  }
  if (status === 'deleted' && view === 'current') {
    return 'File deleted'
  }
  return null
}
