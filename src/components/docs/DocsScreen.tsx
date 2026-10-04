import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useSearchParams } from 'react-router-dom'

import { getIndexStatus, type IndexStatus } from '../../api/codeIndex'
import { getDoc, searchDocs, type DocSearchEngine, type DocSearchResult } from '../../api/docs'
import { useIndexStore } from '../../state/indexStore'
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
  const { files, loading, error } = useWorkspaceDocs(workspaceId)
  const [searchParams, setSearchParams] = useSearchParams()
  const urlFile = searchParams.get('file')
  const [selected, setSelected] = useState<string | null>(
    () => urlFile ?? lastValidPath(workspaceId)
  )
  // The first visit restores the last page into the URL with replace, so it
  // does not add a history entry. After that the URL is the source of truth
  // and the side buttons walk it.
  const seed = useRef<'idle' | 'writing' | 'done'>('idle')
  useLayoutEffect(() => {
    if (seed.current === 'idle') {
      seed.current = selected && !urlFile ? 'writing' : 'done'
      if (seed.current === 'writing' && selected) {
        setSearchParams({ file: selected }, { replace: true })
      }
      return
    }
    if (seed.current === 'writing') {
      if (urlFile) {
        seed.current = 'done'
      }
      return
    }
    setSelected(urlFile)
  }, [urlFile, selected, setSearchParams])

  function openDocument(path: string) {
    if (path === selected) {
      return
    }
    setSearchParams({ file: path })
  }
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

  // Search. The draft drives the field, the committed query drives the
  // request, and results replace the tree while a query is active.
  const [draft, setDraft] = useState('')
  const [query, setQuery] = useState('')
  const [result, setResult] = useState<DocSearchResult | null>(null)
  const [searchError, setSearchError] = useState<string | null>(null)
  const [searching, setSearching] = useState(false)
  const [engine, setEngine] = useState<DocSearchEngine>('semantic')
  const [polled, setPolled] = useState<IndexStatus | null>(null)
  const debounceRef = useRef<number | null>(null)
  const abortRef = useRef<AbortController | null>(null)
  const engineRef = useRef(engine)
  useEffect(() => {
    engineRef.current = engine
  }, [engine])
  const workspaceRef = useRef(workspaceId)
  const queryRef = useRef(query)
  useEffect(() => {
    workspaceRef.current = workspaceId
    queryRef.current = query
  }, [workspaceId, query])

  function clearScheduledSearch() {
    if (debounceRef.current !== null) {
      window.clearTimeout(debounceRef.current)
      debounceRef.current = null
    }
  }

  function abortSearch() {
    abortRef.current?.abort()
    abortRef.current = null
  }

  // One request at a time. A newer call aborts the previous one.
  function beginSearch(q: string) {
    const workspace = workspaceRef.current
    if (!workspace || !q) {
      return
    }
    abortSearch()
    const controller = new AbortController()
    abortRef.current = controller
    queryRef.current = q
    setQuery(q)
    setSearching(true)
    void searchDocs(workspace, q, undefined, controller.signal, engineRef.current)
      .then((found) => {
        if (controller.signal.aborted) {
          return
        }
        setResult(found)
        setSearchError(null)
        setSearching(false)
        if (found.index.state === 'ready') {
          setPolled(null)
        }
      })
      .catch((err: unknown) => {
        if (controller.signal.aborted) {
          return
        }
        setSearchError(err instanceof Error ? err.message : 'Failed to search docs')
        setSearching(false)
      })
  }

  const beginSearchRef = useRef(beginSearch)
  useEffect(() => {
    beginSearchRef.current = beginSearch
  })

  function onDraft(value: string) {
    setDraft(value)
    clearScheduledSearch()
    abortSearch()
    const trimmed = value.trim()
    if (!trimmed) {
      setQuery('')
      queryRef.current = ''
      setResult(null)
      setPolled(null)
      setSearchError(null)
      setSearching(false)
      return
    }
    setSearchError(null)
    debounceRef.current = window.setTimeout(() => {
      debounceRef.current = null
      beginSearchRef.current(trimmed)
    }, 500)
  }

  function onEngine(next: DocSearchEngine) {
    engineRef.current = next
    setEngine(next)
    clearScheduledSearch()
    abortSearch()
    const trimmed = draft.trim()
    if (!trimmed) {
      setSearching(false)
      return
    }
    beginSearchRef.current(trimmed)
  }

  // Index refresh. A keystroke's wait is left alone so that text stays the one that is sent.
  function refreshSearch() {
    if (debounceRef.current !== null) {
      return
    }
    beginSearchRef.current(queryRef.current)
  }

  useEffect(() => {
    return () => {
      clearScheduledSearch()
      abortSearch()
    }
  }, [])

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

  // The index frames on the event stream follow the session's stream, which a
  // docs visit does not open, so poll the status while the index is still
  // being built. When the state moves on, re-run the search: the hits are
  // stale until it does.
  const building = result !== null && result.engine !== 'ripgrep' && result.index.state !== 'ready'
  useEffect(() => {
    if (!workspaceId || !query || !building || !result) {
      return
    }
    let cancelled = false
    let seen = result.index.state
    const timer = window.setInterval(() => {
      void getIndexStatus(workspaceId)
        .then((status) => {
          if (cancelled) {
            return
          }
          setPolled(status)
          if (status.state !== seen) {
            seen = status.state
            refreshSearch()
          }
        })
        .catch(() => {
          // Keep the last notice; the next tick tries again.
        })
    }, 2000)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [workspaceId, query, building, result])

  // Resume is the same control the sidebar's index line offers.
  async function resumeIndex() {
    await useIndexStore.getState().setPaused(false)
    refreshSearch()
  }

  const tree = buildFileTree(files.map((file) => file.path))
  const selectedError = contentError?.path === selected ? contentError.message : null
  // The freshest status while the index is still being built; nothing once it
  // is ready.
  const notice =
    result && result.engine !== 'ripgrep' && result.index.state !== 'ready'
      ? (polled ?? result.index)
      : null

  function noticeText(status: IndexStatus): string {
    switch (status.state) {
      case 'indexing':
        return `Indexing ${status.files_done}/${status.files_total} — results may be incomplete`
      case 'downloading':
        return 'Preparing search…'
      case 'paused':
        return 'Search index paused.'
      case 'failed':
        return 'Search index failed.'
      default:
        return ''
    }
  }

  const canResume = notice !== null && (notice.state === 'paused' || notice.state === 'failed')
  const trimmedDraft = draft.trim()
  // The half-second pause and the request itself both count as pending.
  const searchPending = trimmedDraft !== '' && (searching || trimmedDraft !== query)

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
      {files.length > 0 ? (
        <header className={styles.header}>
          <div className={styles.searchWrap}>
            <input
              className={styles.search}
              type="search"
              value={draft}
              placeholder="Search documentation…"
              aria-label="Search documentation"
              aria-busy={searchPending || undefined}
              onChange={(event) => onDraft(event.target.value)}
            />
            {searchPending ? <span className={styles.spinner} aria-hidden /> : null}
          </div>
          <div className={styles.engines} role="group" aria-label="Search engine">
            <button
              type="button"
              className={styles.engine}
              aria-pressed={engine === 'semantic'}
              onClick={() => onEngine('semantic')}
            >
              Semantic
            </button>
            <button
              type="button"
              className={styles.engine}
              aria-pressed={engine === 'ripgrep'}
              onClick={() => onEngine('ripgrep')}
            >
              Text
            </button>
          </div>
        </header>
      ) : null}
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
          {trimmedDraft ? (
            <div className={styles.results}>
              {notice ? (
                <p className={styles.notice} role="status">
                  {noticeText(notice)}
                  {canResume ? (
                    <button
                      type="button"
                      className={styles.resume}
                      onClick={() => void resumeIndex()}
                    >
                      Resume
                    </button>
                  ) : null}
                </p>
              ) : null}
              {searchError ? (
                <p className={styles.hint} role="alert">
                  {searchError}
                </p>
              ) : searchPending && (!result || result.query !== trimmedDraft) ? (
                <p className={styles.hint} role="status">
                  <span className={styles.spinner} aria-hidden />
                  Searching…
                </p>
              ) : !result || result.hits.length === 0 ? (
                <p className={styles.hint}>No matches for “{trimmedDraft}”.</p>
              ) : (
                <ul className={styles.hits}>
                  {result.hits.map((hit) => (
                    <li key={`${hit.path}:${hit.start_line}`}>
                      <button
                        type="button"
                        className={styles.hit}
                        onClick={() => openDocument(hit.path)}
                      >
                        <span className={styles.hitTitle}>
                          {hit.title || hit.path.split('/').pop()}
                        </span>
                        <span className={styles.hitPath}>{hit.path}</span>
                        <span className={styles.hitSnippet}>{hit.snippet}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : (
            <DocTree
              nodes={tree}
              selected={selected}
              collapsed={collapsed}
              onSelect={openDocument}
              onToggle={toggle}
            />
          )}
          <div className={styles.viewer} ref={viewerRef}>
            {selectedError ? (
              <p className={styles.message} role="alert">
                {selectedError}
              </p>
            ) : selected === null || content === null ? (
              <div className={styles.placeholder}>Select a document to open it.</div>
            ) : (
              <div className={styles.sheet}>
                <AssistantMarkdown
                  text={content}
                  document
                  docPath={selected}
                  onDocLink={openDocument}
                />
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
