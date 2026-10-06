import { memo, useEffect, useState } from 'react'

import { renderMermaid } from './mermaid'
import styles from './AssistantMarkdown.module.css'

/**
 * One fenced `mermaid` block. The source fence stays on screen until the SVG is
 * ready, and stays on screen for good when mermaid cannot parse it.
 *
 * `lines` is the fence's raw source line span in document mode, stamped as
 * `data-md-lines` on the rendered block so the viewer's line-attach hover can
 * target the diagram. Absent in chat.
 */
export const MermaidDiagram = memo(function MermaidDiagram({
  source,
  lines
}: {
  source: string
  lines?: string
}) {
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
      <pre className={styles.diagramSource} data-md-lines={lines}>
        <code>{source}</code>
      </pre>
    )
  }

  return (
    <div className={styles.diagram} data-md-lines={lines}>
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
