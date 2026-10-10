/** @jsxImportSource solid-js */
import { For, Show } from 'solid-js'

import type { DocTreeNode } from './docsTree'
import styles from './DocTree.module.css'

export function DocTree(props: {
  nodes: DocTreeNode[]
  selected: string | null
  collapsed: ReadonlySet<string>
  onSelect: (path: string) => void
  onToggle: (path: string) => void
}) {
  return (
    <nav class={styles.tree} aria-label="Documentation">
      <TreeLevel
        nodes={props.nodes}
        depth={0}
        selected={props.selected}
        collapsed={props.collapsed}
        onSelect={props.onSelect}
        onToggle={props.onToggle}
      />
    </nav>
  )
}

function TreeLevel(props: {
  nodes: DocTreeNode[]
  depth: number
  selected: string | null
  collapsed: ReadonlySet<string>
  onSelect: (path: string) => void
  onToggle: (path: string) => void
}) {
  return (
    <ul class={styles.list}>
      <For each={props.nodes}>
        {(node) => {
          const indent = { 'padding-left': `${12 + props.depth * 14}px` }
          return (
            <Show
              when={node.kind === 'dir'}
              fallback={
                <li>
                  <button
                    type="button"
                    class={node.path === props.selected ? styles.fileActive : styles.file}
                    style={indent}
                    aria-current={node.path === props.selected ? 'true' : undefined}
                    onClick={() => props.onSelect(node.path)}
                  >
                    {node.name}
                  </button>
                </li>
              }
            >
              <li>
                <button
                  type="button"
                  class={dirClass(node, props.collapsed.has(node.path))}
                  style={indent}
                  aria-expanded={!props.collapsed.has(node.path)}
                  onClick={() => props.onToggle(node.path)}
                >
                  <span class={styles.caret} aria-hidden="true">
                    {props.collapsed.has(node.path) ? '▸' : '▾'}
                  </span>
                  {node.name}
                </button>
                <Show when={!props.collapsed.has(node.path)}>
                  <TreeLevel
                    nodes={node.children}
                    depth={props.depth + 1}
                    selected={props.selected}
                    collapsed={props.collapsed}
                    onSelect={props.onSelect}
                    onToggle={props.onToggle}
                  />
                </Show>
              </li>
            </Show>
          )
        }}
      </For>
    </ul>
  )
}

function dirClass(node: DocTreeNode, closed: boolean): string {
  if (node.ignored) {
    return closed ? styles.dirIgnoredClosed : styles.dirIgnoredOpen
  }
  return closed ? styles.dirClosed : styles.dirOpen
}
