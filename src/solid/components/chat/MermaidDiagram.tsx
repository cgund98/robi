/** @jsxImportSource solid-js */
import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

import { renderMermaid } from '../../../components/chat/mermaid'
import styles from '../../../components/chat/AssistantMarkdown.module.css'

export function MermaidDiagram(props: { source: string; lines?: string }) {
  const [result, setResult] = createSignal<{ source: string; svg: string } | null>(null)

  createEffect(() => {
    const source = props.source
    let active = true
    void renderMermaid(source).then(
      (svg) => {
        if (active) {
          setResult({ source, svg })
        }
      },
      () => {
        // No result: the source fence is the fallback.
      }
    )
    onCleanup(() => {
      active = false
    })
  })

  const svg = () => (result()?.source === props.source ? result()!.svg : null)

  return (
    <Show
      when={svg()}
      fallback={
        <pre class={styles.diagramSource} data-md-lines={props.lines}>
          <code>{props.source}</code>
        </pre>
      }
    >
      {(markup) => (
        <div class={styles.diagram} data-md-lines={props.lines}>
          <div class={styles.diagramSvg} role="img" aria-label="Diagram" innerHTML={markup()} />
          <span class={styles.srOnly} data-find-ignore>
            {props.source}
          </span>
        </div>
      )}
    </Show>
  )
}
