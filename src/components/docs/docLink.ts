/**
 * Workspace-relative markdown path a relative link points at, or null when
 * the href is absolute, a fragment, or not a markdown file.
 */
export function resolveMarkdownLink(fromPath: string, href: string): string | null {
  const trimmed = href.trim()
  if (!trimmed || trimmed.startsWith('#') || /^[a-z][a-z0-9+.-]*:/i.test(trimmed)) {
    return null
  }
  let decoded: string
  try {
    decoded = decodeURI(trimmed)
  } catch {
    return null
  }
  const withoutHash = decoded.split('#')[0]?.split('?')[0] ?? ''
  if (!withoutHash || withoutHash.startsWith('/') || !/\.(md|markdown)$/i.test(withoutHash)) {
    return null
  }
  const parts = fromPath.split('/').slice(0, -1)
  for (const segment of withoutHash.split('/')) {
    if (!segment || segment === '.') {
      continue
    }
    if (segment === '..') {
      if (parts.length === 0) {
        return null
      }
      parts.pop()
      continue
    }
    parts.push(segment)
  }
  return parts.join('/')
}
