/** @jsxImportSource solid-js */
import { For, Show } from 'solid-js'

import type { TreeNode } from '../../../components/review/tree'
import styles from '../../../components/review/FileTree.module.css'

export function FileTree(props: {
  nodes: TreeNode[]
  selected: string | null
  onSelect: (path: string) => void
}) {
  return (
    <nav class={styles.tree} aria-label="Changed files">
      <TreeLevel
        nodes={props.nodes}
        depth={0}
        selected={props.selected}
        onSelect={props.onSelect}
      />
    </nav>
  )
}

function TreeLevel(props: {
  nodes: TreeNode[]
  depth: number
  selected: string | null
  onSelect: (path: string) => void
}) {
  return (
    <ul class={styles.list}>
      <For each={props.nodes}>
        {(node) => (
          <li>
            <Show
              when={node.kind === 'dir'}
              fallback={
                <button
                  type="button"
                  class={node.path === props.selected ? styles.fileActive : styles.file}
                  style={{ 'padding-left': `${12 + props.depth * 14}px` }}
                  aria-current={node.path === props.selected ? 'true' : undefined}
                  onClick={() => props.onSelect(node.path)}
                >
                  {node.name}
                </button>
              }
            >
              <div class={styles.dir} style={{ 'padding-left': `${12 + props.depth * 14}px` }}>
                {node.name}
              </div>
              <TreeLevel
                nodes={node.kind === 'dir' ? node.children : []}
                depth={props.depth + 1}
                selected={props.selected}
                onSelect={props.onSelect}
              />
            </Show>
          </li>
        )}
      </For>
    </ul>
  )
}
