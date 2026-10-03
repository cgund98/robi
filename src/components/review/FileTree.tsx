import type { TreeNode } from './tree'
import styles from './FileTree.module.css'

type FileTreeProps = {
  nodes: TreeNode[]
  selected: string | null
  onSelect: (path: string) => void
}

export function FileTree({ nodes, selected, onSelect }: FileTreeProps) {
  return (
    <nav className={styles.tree} aria-label="Changed files">
      <TreeLevel nodes={nodes} depth={0} selected={selected} onSelect={onSelect} />
    </nav>
  )
}

function TreeLevel({ nodes, depth, selected, onSelect }: FileTreeProps & { depth: number }) {
  return (
    <ul className={styles.list}>
      {nodes.map((node) => (
        <li key={`${node.kind}:${node.path}`}>
          {node.kind === 'dir' ? (
            <>
              <div className={styles.dir} style={{ paddingLeft: 12 + depth * 14 }}>
                {node.name}
              </div>
              <TreeLevel
                nodes={node.children}
                depth={depth + 1}
                selected={selected}
                onSelect={onSelect}
              />
            </>
          ) : (
            <button
              type="button"
              className={node.path === selected ? styles.fileActive : styles.file}
              style={{ paddingLeft: 12 + depth * 14 }}
              aria-current={node.path === selected ? 'true' : undefined}
              onClick={() => onSelect(node.path)}
            >
              {node.name}
            </button>
          )}
        </li>
      ))}
    </ul>
  )
}
