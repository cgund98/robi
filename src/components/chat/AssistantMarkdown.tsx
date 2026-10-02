import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

import styles from './AssistantMarkdown.module.css'

type AssistantMarkdownProps = {
  text: string
}

export function AssistantMarkdown({ text }: AssistantMarkdownProps) {
  return (
    <div className={styles.markdown}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a href={href} target="_blank" rel="noreferrer">
              {children}
            </a>
          )
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  )
}
