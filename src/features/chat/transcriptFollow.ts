/** Distance from the bottom of a scrollport. Zero means the end is in view. */
export function distanceFromBottom(node: {
  scrollHeight: number
  scrollTop: number
  clientHeight: number
}): number {
  return node.scrollHeight - node.scrollTop - node.clientHeight
}

/**
 * Whether the transcript should stay pinned after a scroll event.
 * A taller list, such as a tool request opening, is not the reader leaving the end.
 */
export function pinnedAfterScroll(input: {
  pinned: boolean
  scrollTop: number
  previousScrollTop: number
  scrollHeight: number
  previousScrollHeight: number
  clientHeight: number
  threshold: number
}): boolean {
  if (distanceFromBottom(input) <= input.threshold) {
    return true
  }
  if (input.pinned && input.scrollHeight > input.previousScrollHeight + 1) {
    return true
  }
  if (input.scrollTop < input.previousScrollTop - 1) {
    return false
  }
  return input.pinned
}
