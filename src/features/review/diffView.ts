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

export type ReviewChunkHunk = {
  id: string
  /** Index inside the chunk's lines of that hunk's first changed line. */
  firstChange: number
}

export type ReviewChunk = {
  hunks: ReviewChunkHunk[]
  lines: ReviewLine[]
}

/**
 * Visual blocks, split on gaps. Each block carries every logical hunk that lands
 * inside it, with the index of that hunk's first changed line, so each hunk can
 * get its own approve or reject control. A block can hold more than one hunk.
 */
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
    hunks: hunks.flatMap((hunk) => {
      const firstChange = group.findIndex((line) => hunkTouchesLine(hunk, line))
      return firstChange < 0 ? [] : [{ id: hunk.id, firstChange }]
    })
  }))
}

function hunkTouchesLine(hunk: ReviewHunk, line: ReviewLine): boolean {
  if (line.kind === 'delete' && line.old_line != null && hunk.old_count > 0) {
    return line.old_line > hunk.old_start && line.old_line <= hunk.old_start + hunk.old_count
  }
  if (line.kind === 'insert' && line.new_line != null && hunk.new_count > 0) {
    return line.new_line > hunk.new_start && line.new_line <= hunk.new_start + hunk.new_count
  }
  return false
}

/**
 * The review column keeps three lines of context and a gap for the rest.
 * The file viewer wants every line, with the same inserts and deletions.
 */
export function expandReviewLines(
  lines: ReviewLine[],
  baseline: string,
  current: string
): ReviewLine[] {
  const before = splitFile(baseline)
  const after = splitFile(current)
  const shown = lines.filter((line) => line.kind !== 'gap')
  const out: ReviewLine[] = []
  let oldNext = 1
  let newNext = 1

  for (const line of shown) {
    if (line.kind === 'delete' && line.old_line != null) {
      const skip = line.old_line - oldNext
      ;({ oldNext, newNext } = fillEqual(
        out,
        before,
        after,
        oldNext,
        newNext,
        line.old_line,
        newNext + skip
      ))
    } else if (line.kind === 'insert' && line.new_line != null) {
      const skip = line.new_line - newNext
      ;({ oldNext, newNext } = fillEqual(
        out,
        before,
        after,
        oldNext,
        newNext,
        oldNext + skip,
        line.new_line
      ))
    } else if (line.kind === 'context' && line.old_line != null && line.new_line != null) {
      ;({ oldNext, newNext } = fillEqual(
        out,
        before,
        after,
        oldNext,
        newNext,
        line.old_line,
        line.new_line
      ))
    }
    out.push(line)
    if (line.old_line != null) {
      oldNext = line.old_line + 1
    }
    if (line.new_line != null) {
      newNext = line.new_line + 1
    }
  }

  fillEqual(out, before, after, oldNext, newNext, before.length + 1, after.length + 1)
  return out
}

function fillEqual(
  out: ReviewLine[],
  baseline: string[],
  current: string[],
  oldNext: number,
  newNext: number,
  oldStop: number,
  newStop: number
): { oldNext: number; newNext: number } {
  while (oldNext < oldStop && newNext < newStop) {
    out.push({
      kind: 'context',
      text: current[newNext - 1] ?? baseline[oldNext - 1] ?? '',
      old_line: oldNext,
      new_line: newNext
    })
    oldNext += 1
    newNext += 1
  }
  return { oldNext, newNext }
}

function splitFile(text: string): string[] {
  if (text === '') {
    return []
  }
  const lines = text.split(/\r?\n/)
  if (lines[lines.length - 1] === '') {
    lines.pop()
  }
  return lines
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
