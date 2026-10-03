export type TreeNode = {
  name: string
  path: string
  kind: 'dir' | 'file'
  children: TreeNode[]
}

function compareName(left: string, right: string): number {
  return left.localeCompare(right, undefined, { sensitivity: 'base' })
}

function sortLevel(nodes: TreeNode[]): TreeNode[] {
  const dirs = nodes
    .filter((node) => node.kind === 'dir')
    .sort((a, b) => compareName(a.name, b.name))
  const files = nodes
    .filter((node) => node.kind === 'file')
    .sort((a, b) => compareName(a.name, b.name))
  return [...dirs, ...files]
}

function sortDeep(node: TreeNode): void {
  node.children = sortLevel(node.children)
  for (const child of node.children) {
    sortDeep(child)
  }
}

/** Directories first, then files, each group alphabetical at every level. */
export function buildFileTree(paths: string[]): TreeNode[] {
  const root: TreeNode = { name: '', path: '', kind: 'dir', children: [] }
  for (const filePath of paths) {
    const parts = filePath.split('/').filter((part) => part.length > 0)
    let cursor = root
    let acc = ''
    parts.forEach((part, index) => {
      acc = acc ? `${acc}/${part}` : part
      const kind = index === parts.length - 1 ? 'file' : 'dir'
      let child = cursor.children.find((item) => item.name === part && item.kind === kind)
      if (!child) {
        child = { name: part, path: acc, kind, children: [] }
        cursor.children.push(child)
      }
      cursor = child
    })
  }
  sortDeep(root)
  return root.children
}

function walk(nodes: TreeNode[], out: string[]): void {
  for (const node of nodes) {
    if (node.kind === 'file') {
      out.push(node.path)
    } else {
      walk(node.children, out)
    }
  }
}

/** File paths in the same order the tree renders them. */
export function filesInTreeOrder(paths: string[]): string[] {
  const ordered: string[] = []
  walk(buildFileTree(paths), ordered)
  return ordered
}
