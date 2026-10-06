/**
 * Literal, in-document search for the docs viewer.
 *
 * Matches are found by walking the text nodes of the rendered document and
 * building `Range` objects. The rendered DOM is never mutated: the ranges
 * are registered with the CSS Custom Highlight API, so a re-render of the
 * document cannot corrupt the search state.
 */

/** Match offsets in one string, as `[start, end)` in original coordinates. */
export type Match = {
  start: number
  end: number
}

/** The registry names the stylesheet styles. */
export const FIND_HIGHLIGHT = 'robi-doc-find'
export const FIND_CURRENT_HIGHLIGHT = 'robi-doc-find-current'

/**
 * Regions that are not the prose the reader sees, so find leaves them alone.
 * `svg` is diagram text, `aria-hidden` is a decorative or duplicate copy, and
 * `data-find-ignore` marks a hidden copy that a component adds itself.
 */
const SKIP_SELECTOR = 'script, style, svg, [aria-hidden="true"], [data-find-ignore]'

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/**
 * Every non-overlapping occurrence of `query` in `text`, in order.
 *
 * Literal: the query is escaped, so `.` matches a dot. The `u` flag with a
 * walked `lastIndex` keeps offsets in the original string under either case
 * flag, which a lowercased copy would not.
 */
export function matchRanges(text: string, query: string, caseSensitive: boolean): Match[] {
  if (!query) {
    return []
  }
  const pattern = new RegExp(escapeRegExp(query), caseSensitive ? 'gu' : 'giu')
  const matches: Match[] = []
  for (let found = pattern.exec(text); found !== null; found = pattern.exec(text)) {
    matches.push({ start: found.index, end: found.index + found[0].length })
    // A zero-width match cannot happen: the query is non-empty and escaped.
  }
  return matches
}

/**
 * Every match in `root`'s text nodes, in document order.
 *
 * Matching is per text node: a phrase broken by an inline element (say, an
 * emphasised word) is two text nodes and does not match as one hit.
 */
export function collectMatches(root: ParentNode, query: string, caseSensitive: boolean): Range[] {
  if (!query || !root.ownerDocument) {
    return []
  }
  const doc = root.ownerDocument
  const ranges: Range[] = []
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => {
      const parent = node.parentElement
      if (parent && parent.closest(SKIP_SELECTOR)) {
        return NodeFilter.FILTER_REJECT
      }
      return NodeFilter.FILTER_ACCEPT
    }
  })
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const text = node.nodeValue ?? ''
    for (const match of matchRanges(text, query, caseSensitive)) {
      const range = doc.createRange()
      range.setStart(node, match.start)
      range.setEnd(node, match.end)
      ranges.push(range)
    }
  }
  return ranges
}

/** Whether this engine can paint matches with the CSS Custom Highlight API. */
export function supportsHighlights(): boolean {
  return typeof Highlight === 'function' && typeof CSS !== 'undefined' && 'highlights' in CSS
}

/**
 * Register the matches with the highlight registry: every match under one name,
 * the active match under another. Without the API support, this is a no-op and
 * the bar still counts and scrolls.
 */
export function applyHighlights(ranges: Range[], current: number): void {
  if (!supportsHighlights()) {
    return
  }
  const all = new Highlight(...ranges)
  CSS.highlights.set(FIND_HIGHLIGHT, all)
  const active = ranges[current]
  if (active) {
    CSS.highlights.set(FIND_CURRENT_HIGHLIGHT, new Highlight(active))
  } else {
    CSS.highlights.delete(FIND_CURRENT_HIGHLIGHT)
  }
}

/** Remove both highlight names, leaving the document styled as it was. */
export function clearHighlights(): void {
  if (!supportsHighlights()) {
    return
  }
  CSS.highlights.delete(FIND_HIGHLIGHT)
  CSS.highlights.delete(FIND_CURRENT_HIGHLIGHT)
}

/**
 * Scroll a match to the middle of the viewer. The two rects are both
 * viewport-relative, so their difference is the match's offset inside the pane.
 * A match still waiting to lay out has an empty rect and is left alone.
 */
export function scrollRangeIntoView(viewer: HTMLElement, range: Range): void {
  const rect = range.getBoundingClientRect()
  if (rect.width === 0 && rect.height === 0) {
    return
  }
  const view = viewer.getBoundingClientRect()
  const offset = rect.top - view.top + rect.height / 2 - viewer.clientHeight / 2
  viewer.scrollTop += offset
}
