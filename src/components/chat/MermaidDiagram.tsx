import { memo, useEffect, useState } from 'react'

import { renderMermaid } from './mermaid'
import styles from './AssistantMarkdown.module.css'

/**
 * One fenced `mermaid` block. The source fence stays on screen until the SVG is
 * ready, and stays on screen for good when mermaid cannot parse it.
 */
export const MermaidDiagram = memo(function MermaidDiagram({ source }: { source: string }) {
  // The SVG is paired with the source it came from, so a source change falls
  // back to the fence immediately without a reset render.
  const [result, setResult] = useState<{ source: string; svg: string } | null>(null)

  useEffect(() => {
    let active = true
    renderMermaid(source).then(
      (svg) => {
        if (active) {
          setResult({ source, svg })
        }
      },
      () => {
        // No result: the source fence is the fallback.
      }
    )
    return () => {
      active = false
    }
  }, [source])

  const svg = result?.source === source ? result.svg : null

  if (svg === null) {
    return (
      <pre className={styles.diagramSource}>
        <code>{source}</code>
      </pre>
    )
  }

  return (
    <div className={styles.diagram}>
      <div
        className={styles.diagramSvg}
        role="img"
        aria-label="Diagram"
        dangerouslySetInnerHTML={{ __html: svg }}
      />
      <span className={styles.srOnly} data-find-ignore>
        {source}
      </span>
    </div>
  )
})
