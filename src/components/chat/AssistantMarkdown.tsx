import { memo, type ReactNode } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

import styles from './AssistantMarkdown.module.css'

type AssistantMarkdownProps = {
  text: string
  /** Document headings step down by level. Chat keeps one size. */
  document?: boolean
}

const remarkPlugins = [remarkGfm]

function MarkdownLink({ href, children }: { href?: string; children?: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noreferrer">
      {children}
    </a>
  )
}

const components = { a: MarkdownLink }

export const AssistantMarkdown = memo(function AssistantMarkdown({
  text,
  document = false
}: AssistantMarkdownProps) {
  return (
    <div className={document ? `${styles.markdown} ${styles.document}` : styles.markdown}>
      <ReactMarkdown remarkPlugins={remarkPlugins} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  )
})
