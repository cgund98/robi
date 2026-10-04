import { Children, isValidElement, memo, useMemo, type ReactNode } from 'react'
import ReactMarkdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'

import { resolveMarkdownLink } from '../docs/docLink'
import { MermaidDiagram } from './MermaidDiagram'
import styles from './AssistantMarkdown.module.css'

type AssistantMarkdownProps = {
  text: string
  /** Document headings step down by level. Chat keeps one size. */
  document?: boolean
  /**
   * When set, a relative markdown link opens that workspace path instead of
   * a new tab. `docPath` is the open file the link is resolved against.
   */
  docPath?: string
  onDocLink?: (path: string) => void
}

const remarkPlugins = [remarkGfm]

function MarkdownLink({
  href,
  children,
  docPath,
  onDocLink
}: {
  href?: string
  children?: ReactNode
  docPath?: string
  onDocLink?: (path: string) => void
}) {
  const target = href && docPath && onDocLink ? resolveMarkdownLink(docPath, href) : null
  if (target && onDocLink) {
    return (
      <a
        href={href}
        onClick={(event) => {
          event.preventDefault()
          onDocLink(target)
        }}
      >
        {children}
      </a>
    )
  }
  return (
    <a href={href} target="_blank" rel="noreferrer">
      {children}
    </a>
  )
}

function MarkdownCode({ className, children }: { className?: string; children?: ReactNode }) {
  return <code className={className}>{children}</code>
}

function MarkdownPre({ children }: { children?: ReactNode }) {
  // A mermaid fence is a diagram, not a code block. The code component is
  // still this element's child here — it has not rendered — so returning it
  // unchanged leaves the diagram inside `<pre>`, and the code-block surface
  // paints a second rounded box around the diagram.
  const only = Children.count(children) === 1 ? Children.only(children) : null
  if (
    isValidElement<{ className?: string; children?: ReactNode }>(only) &&
    only.type === MarkdownCode
  ) {
    const language = /language-([\w-]+)/.exec(only.props.className ?? '')?.[1]?.toLowerCase()
    if (language === 'mermaid' && typeof only.props.children === 'string') {
      return <MermaidDiagram source={only.props.children.replace(/\n$/, '')} />
    }
  }
  return <pre>{children}</pre>
}

const components: Components = { code: MarkdownCode, pre: MarkdownPre }

export const AssistantMarkdown = memo(function AssistantMarkdown({
  text,
  document = false,
  docPath,
  onDocLink
}: AssistantMarkdownProps) {
  const rendered = useMemo<Components>(
    () => ({
      ...components,
      a: (props) => <MarkdownLink {...props} docPath={docPath} onDocLink={onDocLink} />
    }),
    [docPath, onDocLink]
  )
  return (
    <div className={document ? `${styles.markdown} ${styles.document}` : styles.markdown}>
      <ReactMarkdown remarkPlugins={remarkPlugins} components={rendered}>
        {text}
      </ReactMarkdown>
    </div>
  )
})
