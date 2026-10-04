import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { getDoc } from '../../api/docs'
import {
  lastValidPath,
  readCollapsed,
  readContent,
  readScroll,
  recordCollapsed,
  recordPath,
  recordScroll,
  writeContent
} from '../../app/docsCache'
import { useWorkspaceDocs } from '../../app/useWorkspaceDocs'
import { AssistantMarkdown } from '../chat/AssistantMarkdown'
import { buildFileTree } from '../review/tree'
import { DocTree } from './DocTree'
import styles from './DocsScreen.module.css'

type DocsScreenProps = {
  workspaceId: string | null
}

type DocError = {
  path: string
  message: string
}

export function DocsScreen({ workspaceId }: DocsScreenProps) {
  const navigate = useNavigate()
  const { files, loading, error } = useWorkspaceDocs(workspaceId)
  const [selected, setSelected] = useState<string | null>(() => lastValidPath(workspaceId))
  const [content, setContent] = useState<string | null>(() =>
    workspaceId && selected ? (readContent(workspaceId, selected) ?? null) : null
  )
  const [contentPath, setContentPath] = useState(selected)
  const [contentError, setContentError] = useState<DocError | null>(null)
  if (contentPath !== selected) {
    setContentPath(selected)
    setContent(workspaceId && selected ? (readContent(workspaceId, selected) ?? null) : null)
  }
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() =>
    workspaceId ? new Set(readCollapsed(workspaceId)) : new Set()
  )
  const viewerRef = useRef<HTMLDivElement>(null)
  const scrollRef = useRef(0)

  // The chosen document is remembered so the next visit reopens it.
  useEffect(() => {
    if (workspaceId) {
      recordPath(workspaceId, selected)
    }
  }, [workspaceId, selected])

  // Folded directories are remembered too.
  useEffect(() => {
    if (workspaceId) {
      recordCollapsed(workspaceId, collapsed)
    }
  }, [workspaceId, collapsed])

  useEffect(() => {
    if (!workspaceId || !selected) {
      return
    }
    let cancelled = false
    void getDoc(workspaceId, selected)
      .then((doc) => {
        if (!cancelled) {
          writeContent(workspaceId, selected, doc.content)
          setContent(doc.content)
          setContentError(null)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setContentError({
            path: selected,
            message: err instanceof Error ? err.message : 'Failed to load document'
          })
        }
      })
    return () => {
      cancelled = true
    }
  }, [workspaceId, selected])

  // Restore the saved scroll offset after the document paints.
  useLayoutEffect(() => {
    const viewer = viewerRef.current
    if (!viewer || !workspaceId || !selected || content === null) {
      return
    }
    viewer.scrollTop = readScroll(workspaceId, selected)
    scrollRef.current = viewer.scrollTop
  }, [workspaceId, selected, content])

  // Save the scroll offset, including on the unmount that leaves the page.
  useEffect(() => {
    const viewer = viewerRef.current
    if (!viewer || !workspaceId || !selected) {
      return
    }
    const path = selected
    const onScroll = () => {
      scrollRef.current = viewer.scrollTop
    }
    viewer.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      viewer.removeEventListener('scroll', onScroll)
      recordScroll(workspaceId, path, scrollRef.current)
    }
  }, [workspaceId, selected])

  const tree = buildFileTree(files.map((file) => file.path))
  const selectedError = contentError?.path === selected ? contentError.message : null

  function toggle(path: string) {
    setCollapsed((current) => {
      const next = new Set(current)
      if (next.has(path)) {
        next.delete(path)
      } else {
        next.add(path)
      }
      return next
    })
  }

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <button type="button" className={styles.back} onClick={() => navigate('/')}>
          ← Workspace
        </button>
        {files.length > 0 ? (
          <span className={styles.count}>
            {files.length} {files.length === 1 ? 'page' : 'pages'}
          </span>
        ) : null}
      </header>
      {!workspaceId ? (
        <p className={styles.message}>No workspace is open.</p>
      ) : error ? (
        <p className={styles.message} role="alert">
          {error}
        </p>
      ) : loading ? (
        <p className={styles.message}>Loading docs…</p>
      ) : files.length === 0 ? (
        <p className={styles.message}>No markdown files in this workspace.</p>
      ) : (
        <div className={styles.body}>
          <DocTree
            nodes={tree}
            selected={selected}
            collapsed={collapsed}
            onSelect={setSelected}
            onToggle={toggle}
          />
          <div className={styles.viewer} ref={viewerRef}>
            {selectedError ? (
              <p className={styles.message} role="alert">
                {selectedError}
              </p>
            ) : selected === null || content === null ? (
              <div className={styles.placeholder}>Select a document to open it.</div>
            ) : (
              <div className={styles.sheet}>
                <AssistantMarkdown text={content} document />
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
