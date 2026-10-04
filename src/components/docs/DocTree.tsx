import type { TreeNode } from '../review/tree'
import styles from './DocTree.module.css'

type DocTreeProps = {
  nodes: TreeNode[]
  selected: string | null
  /** Directory paths the user has folded closed. */
  collapsed: ReadonlySet<string>
  onSelect: (path: string) => void
  onToggle: (path: string) => void
}

export function DocTree({ nodes, selected, collapsed, onSelect, onToggle }: DocTreeProps) {
  return (
    <nav className={styles.tree} aria-label="Documentation">
      <TreeLevel
        nodes={nodes}
        depth={0}
        selected={selected}
        collapsed={collapsed}
        onSelect={onSelect}
        onToggle={onToggle}
      />
    </nav>
  )
}

function TreeLevel({
  nodes,
  depth,
  selected,
  collapsed,
  onSelect,
  onToggle
}: DocTreeProps & { depth: number }) {
  return (
    <ul className={styles.list}>
      {nodes.map((node) => {
        const indent = { paddingLeft: 12 + depth * 14 }
        if (node.kind === 'dir') {
          const open = !collapsed.has(node.path)
          return (
            <li key={`${node.kind}:${node.path}`}>
              <button
                type="button"
                className={styles.dir}
                style={indent}
                aria-expanded={open}
                onClick={() => onToggle(node.path)}
              >
                <span className={styles.caret} aria-hidden>
                  {open ? '▾' : '▸'}
                </span>
                {node.name}
              </button>
              {open ? (
                <TreeLevel
                  nodes={node.children}
                  depth={depth + 1}
                  selected={selected}
                  collapsed={collapsed}
                  onSelect={onSelect}
                  onToggle={onToggle}
                />
              ) : null}
            </li>
          )
        }
        return (
          <li key={`${node.kind}:${node.path}`}>
            <button
              type="button"
              className={node.path === selected ? styles.fileActive : styles.file}
              style={indent}
              aria-current={node.path === selected ? 'true' : undefined}
              onClick={() => onSelect(node.path)}
            >
              {node.name}
            </button>
          </li>
        )
      })}
    </ul>
  )
}
