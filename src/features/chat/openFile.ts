/** True when `path` matches one non-blank glob in `patterns`. */
export function matchesOpenPattern(path: string, patterns: string): boolean {
  for (const line of patterns.split('\n')) {
    const trimmed = line.trim()
    if (trimmed.length === 0) {
      continue
    }
    if (globMatches(trimmed, path)) {
      return true
    }
  }
  return false
}

/** `*` is any run of characters, including `/`. `?` is one character. The match is the whole path. */
function globMatches(pattern: string, path: string): boolean {
  let source = '^'
  for (const ch of pattern) {
    if (ch === '*') {
      source += '.*'
    } else if (ch === '?') {
      source += '.'
    } else {
      source += ch.replace(/[\\^$.*+?()[\]{}|]/g, '\\$&')
    }
  }
  source += '$'
  return new RegExp(source).test(path)
}
