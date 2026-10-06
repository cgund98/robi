/** @jsxImportSource solid-js */
import { SolidMarkdown } from 'solid-markdown'
import remarkGfm from 'remark-gfm'
import { Show } from 'solid-js'
import { Dynamic } from 'solid-js/web'

import { resolveMarkdownLink } from '../../../components/docs/docLink'
import styles from '../../../components/chat/AssistantMarkdown.module.css'
import { MermaidDiagram } from './MermaidDiagram'

const remarkPlugins = [remarkGfm]

type MdNode = {
  position?: { start?: { line?: number }; end?: { line?: number } }
  children?: MdNode[]
  type?: string
  tagName?: string
  value?: string
  properties?: { className?: unknown }
}

function lineSpan(node?: MdNode): string | null {
  const start = node?.position?.start?.line
  if (typeof start !== 'number') {
    return null
  }
  const end = node?.position?.end?.line
  const last = typeof end === 'number' ? end : start
  return last === start ? String(start) : `${start}-${last}`
}

function mermaidSource(node?: MdNode): { source: string; lines?: string } | null {
  const code = node?.children?.find(
    (child): child is MdNode => child.type === 'element' && child.tagName === 'code'
  )
  if (!code) {
    return null
  }
  const className = code.properties?.className
  const classes = Array.isArray(className) ? className.join(' ') : ''
  const language = /language-([\w-]+)/.exec(classes)?.[1]?.toLowerCase()
  if (language !== 'mermaid') {
    return null
  }
  const text = (code.children ?? [])
    .map((child) => (child.type === 'text' ? child.value : ''))
    .join('')
    .replace(/\n$/, '')
  return { source: text, lines: lineSpan(node) ?? undefined }
}

function MarkdownPre(props: { node?: MdNode; children?: unknown }) {
  const diagram = () => mermaidSource(props.node)
  return (
    <Show when={diagram()} fallback={<pre>{props.children as never}</pre>}>
      {(found) => <MermaidDiagram source={found().source} lines={found().lines} />}
    </Show>
  )
}

function MarkdownLink(props: {
  href?: string
  children?: unknown
  docPath?: string
  onDocLink?: (path: string) => void
}) {
  const target = () =>
    props.href && props.docPath && props.onDocLink
      ? resolveMarkdownLink(props.docPath, props.href)
      : null
  return (
    <Show
      when={target()}
      fallback={
        <a href={props.href} target="_blank" rel="noreferrer">
          {props.children as never}
        </a>
      }
    >
      <a
        href={props.href}
        onClick={(event) => {
          event.preventDefault()
          const next = target()
          if (next) {
            props.onDocLink?.(next)
          }
        }}
      >
        {props.children as never}
      </a>
    </Show>
  )
}

function stamped(tag: string) {
  return (block: { node?: MdNode; children?: unknown }) => {
    const span = lineSpan(block.node)
    return (
      <Dynamic component={tag} data-md-lines={span ?? undefined}>
        {block.children as never}
      </Dynamic>
    )
  }
}

export function AssistantMarkdown(props: {
  text: string
  document?: boolean
  docPath?: string
  onDocLink?: (path: string) => void
}) {
  return (
    <div class={props.document ? `${styles.markdown} ${styles.document}` : styles.markdown}>
      <SolidMarkdown
        remarkPlugins={remarkPlugins}
        components={{
          pre: MarkdownPre,
          ...(props.document
            ? {
                p: stamped('p'),
                h1: stamped('h1'),
                h2: stamped('h2'),
                h3: stamped('h3'),
                h4: stamped('h4'),
                h5: stamped('h5'),
                h6: stamped('h6'),
                li: stamped('li'),
                blockquote: stamped('blockquote'),
                tr: stamped('tr'),
                hr: stamped('hr')
              }
            : {}),
          a: (link) => (
            <MarkdownLink href={link.href} docPath={props.docPath} onDocLink={props.onDocLink}>
              {link.children}
            </MarkdownLink>
          )
        }}
      >
        {props.text}
      </SolidMarkdown>
    </div>
  )
}
