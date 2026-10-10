/**
 * Merge and shape docs listings.
 *
 * A recursive listing replaces the scannable tree and keeps descendants of
 * directories it did not fetch. A one-level listing replaces one directory's
 * direct children and keeps grandchildren already cached under those children.
 */

export type DocTreeEntry = {
  path: string
  kind: 'file' | 'directory'
  ignored: boolean
  children_fetched?: boolean | null
}

export type DocTreeNode = {
  name: string
  path: string
  kind: 'dir' | 'file'
  ignored: boolean
  childrenFetched: boolean
  children: DocTreeNode[]
}

export function parentPath(path: string): string {
  const index = path.lastIndexOf('/')
  return index === -1 ? '' : path.slice(0, index)
}

/** A directory whose fetched tree contains no markdown file. */
export function directoryLacksMarkdown(path: string, entries: DocTreeEntry[]): boolean {
  return !entries.some((entry) => entry.kind === 'file' && isStrictDescendant(entry.path, path))
}

export function isStrictDescendant(path: string, dir: string): boolean {
  if (dir === '') {
    return path.length > 0
  }
  return path.startsWith(`${dir}/`)
}

/** Replace `dir`'s direct children. Grandchildren of a child directory stay. */
export function mergeLevel(
  current: DocTreeEntry[],
  dir: string,
  children: DocTreeEntry[],
  expanded: ReadonlySet<string> = new Set()
): DocTreeEntry[] {
  const surviving = children
    .filter((entry) => entry.kind === 'directory')
    .map((entry) => entry.path)
  const next = current.filter((entry) => {
    if (parentPath(entry.path) === dir) {
      return false
    }
    if (!isStrictDescendant(entry.path, dir)) {
      return true
    }
    return surviving.some((child) => isStrictDescendant(entry.path, child))
  })
  const byPath = new Map(next.map((entry) => [entry.path, entry]))
  for (const child of children) {
    const previous = current.find((entry) => entry.path === child.path)
    if (
      child.kind === 'directory' &&
      (expanded.has(child.path) || previous?.children_fetched === true)
    ) {
      byPath.set(child.path, { ...child, children_fetched: true })
    } else {
      byPath.set(child.path, child)
    }
  }
  return [...byPath.values()]
}

/** The one-level response fetched `dir`, so its own node records that. */
export function markFetched(entries: DocTreeEntry[], dir: string): DocTreeEntry[] {
  if (!dir) {
    return entries
  }
  return entries.map((entry) =>
    entry.path === dir && entry.kind === 'directory' ? { ...entry, children_fetched: true } : entry
  )
}

/**
 * Apply a recursive listing. Descendants of directories this response did not
 * fetch stay. A directory the user expanded, or that was already fetched, stays
 * fetched while it is still in the listing. A directory the listing no longer
 * contains is dropped, along with its descendants.
 */
export function mergeRecursive(
  current: DocTreeEntry[],
  incoming: DocTreeEntry[],
  expanded: ReadonlySet<string>
): DocTreeEntry[] {
  const incomingPaths = new Set(incoming.map((entry) => entry.path))
  const stillFetched = new Set(
    incoming
      .filter(
        (entry) =>
          entry.kind === 'directory' &&
          (expanded.has(entry.path) ||
            current.some((item) => item.path === entry.path && item.children_fetched === true))
      )
      .map((entry) => entry.path)
  )
  const kept = current.filter(
    (entry) =>
      !incomingPaths.has(entry.path) &&
      [...stillFetched].some((dir) => isStrictDescendant(entry.path, dir))
  )
  const byPath = new Map<string, DocTreeEntry>()
  for (const entry of kept) {
    byPath.set(entry.path, entry)
  }
  for (const entry of incoming) {
    if (entry.kind === 'directory' && stillFetched.has(entry.path)) {
      byPath.set(entry.path, { ...entry, children_fetched: true })
    } else {
      byPath.set(entry.path, entry)
    }
  }
  return [...byPath.values()]
}

export function sameEntries(current: DocTreeEntry[], next: DocTreeEntry[]): boolean {
  if (current.length !== next.length) {
    return false
  }
  const sortedCurrent = [...current].sort((left, right) => left.path.localeCompare(right.path))
  const sortedNext = [...next].sort((left, right) => left.path.localeCompare(right.path))
  for (let i = 0; i < sortedCurrent.length; i++) {
    const left = sortedCurrent[i]
    const right = sortedNext[i]
    if (
      left.path !== right.path ||
      left.kind !== right.kind ||
      left.ignored !== right.ignored ||
      (left.children_fetched ?? null) !== (right.children_fetched ?? null)
    ) {
      return false
    }
  }
  return true
}

function compareName(left: string, right: string): number {
  return left.localeCompare(right, undefined, { sensitivity: 'base' })
}

function sortLevel(nodes: DocTreeNode[]): DocTreeNode[] {
  const dirs = nodes
    .filter((node) => node.kind === 'dir')
    .sort((a, b) => compareName(a.name, b.name))
  const files = nodes
    .filter((node) => node.kind === 'file')
    .sort((a, b) => compareName(a.name, b.name))
  return [...dirs, ...files]
}

function sortDeep(node: DocTreeNode): void {
  node.children = sortLevel(node.children)
  for (const child of node.children) {
    sortDeep(child)
  }
}

/** Directories that contain a page or an unfetched directory, then files. */
export function buildDocTree(entries: DocTreeEntry[]): DocTreeNode[] {
  const root: DocTreeNode = {
    name: '',
    path: '',
    kind: 'dir',
    ignored: false,
    childrenFetched: true,
    children: []
  }
  const sorted = [...entries].sort((left, right) => left.path.localeCompare(right.path))
  for (const entry of sorted) {
    const parts = entry.path.split('/').filter((part) => part.length > 0)
    let cursor = root
    let acc = ''
    parts.forEach((part, index) => {
      acc = acc ? `${acc}/${part}` : part
      const last = index === parts.length - 1
      const kind = last && entry.kind === 'file' ? 'file' : 'dir'
      let child = cursor.children.find((item) => item.name === part && item.kind === kind)
      if (!child) {
        child = {
          name: part,
          path: acc,
          kind,
          ignored: last ? entry.ignored : false,
          childrenFetched: kind === 'dir' ? (last ? entry.children_fetched === true : true) : true,
          children: []
        }
        cursor.children.push(child)
      } else if (last) {
        child.ignored = entry.ignored
        if (kind === 'dir') {
          child.childrenFetched = entry.children_fetched === true
        }
      }
      cursor = child
    })
  }
  sortDeep(root)
  return root.children
}
