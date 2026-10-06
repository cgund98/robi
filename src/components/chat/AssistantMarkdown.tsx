import { Children, createElement, isValidElement, memo, useMemo, type ReactNode } from 'react'
import ReactMarkdown, { type Components, type ExtraProps } from 'react-markdown'
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

/** The mdast `position` react-markdown hands every component, via `ExtraProps`. */
type BlockProps = ExtraProps & { children?: ReactNode }

/**
 * The raw source line span of a block, as `"42"` or `"40-44"`, or null.
 *
 * react-markdown copies the mdast `position` onto each element's `node` prop,
 * so this is the line in the file the document was rendered from — never a
 * count of rendered elements or laid-out visual lines. Document mode stamps it
 * as `data-md-lines` so the viewer can map a hovered block back to the source.
 */
function lineSpan(node?: ExtraProps['node']): string | null {
  const start = node?.position?.start?.line
  if (typeof start !== 'number') {
    return null
  }
  const end = node?.position?.end?.line
  const last = typeof end === 'number' ? end : start
  return last === start ? String(start) : `${start}-${last}`
}

function lineAttrs(node?: ExtraProps['node']): { 'data-md-lines'?: string } {
  const span = lineSpan(node)
  return span ? { 'data-md-lines': span } : {}
}

/**
 * A document-mode block component that stamps its raw source range.
 *
 * Built at module scope so the identity is stable across renders — the whole
 * component map is held in a `useMemo` keyed on the dynamic `a` override, and a
 * factory called per render would defeat it. `node` is stripped before the
 * spread so it is not written to the DOM.
 */
function withLine(tag: string) {
  const Block = ({ node, ...props }: BlockProps) =>
    createElement(tag, { ...props, ...lineAttrs(node) })
  Block.displayName = `Doc${tag}`
  return Block
}

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

function MarkdownPre({ node, children }: BlockProps) {
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
      return (
        <MermaidDiagram
          source={only.props.children.replace(/\n$/, '')}
          lines={lineSpan(node) ?? undefined}
        />
      )
    }
  }
  return <pre {...lineAttrs(node)}>{children}</pre>
}

const components: Components = { code: MarkdownCode, pre: MarkdownPre }

/**
 * Document-mode overrides. The innermost block under the pointer wins, so a
 * paragraph inside a list item or blockquote carries its own tighter range and
 * a tight list item falls back to its own. Chat mode uses the plain map, so
 * no chat DOM changes.
 */
const documentComponents: Components = {
  ...components,
  p: withLine('p'),
  h1: withLine('h1'),
  h2: withLine('h2'),
  h3: withLine('h3'),
  h4: withLine('h4'),
  h5: withLine('h5'),
  h6: withLine('h6'),
  li: withLine('li'),
  blockquote: withLine('blockquote'),
  tr: withLine('tr'),
  hr: withLine('hr')
}

export const AssistantMarkdown = memo(function AssistantMarkdown({
  text,
  document = false,
  docPath,
  onDocLink
}: AssistantMarkdownProps) {
  const rendered = useMemo<Components>(
    () => ({
      ...(document ? documentComponents : components),
      a: (props) => <MarkdownLink {...props} docPath={docPath} onDocLink={onDocLink} />
    }),
    [document, docPath, onDocLink]
  )
  return (
    <div className={document ? `${styles.markdown} ${styles.document}` : styles.markdown}>
      <ReactMarkdown remarkPlugins={remarkPlugins} components={rendered}>
        {text}
      </ReactMarkdown>
    </div>
  )
})
