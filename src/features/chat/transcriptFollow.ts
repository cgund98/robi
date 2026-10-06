/** Distance from the bottom of a scrollport. Zero means the end is in view. */
export function distanceFromBottom(node: {
  scrollHeight: number
  scrollTop: number
  clientHeight: number
}): number {
  return node.scrollHeight - node.scrollTop - node.clientHeight
}
