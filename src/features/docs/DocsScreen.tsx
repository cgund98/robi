/** @jsxImportSource solid-js */
import { useSearchParams } from '@solidjs/router'
import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js'

import { getIndexStatus, type IndexStatus } from '../../api/codeIndex'
import { getDoc, searchDocs, type DocSearchEngine, type DocSearchResult } from '../../api/docs'
import {
  lastValidPath,
  readCollapsed,
  readContent,
  readExpanded,
  readMode,
  readScroll,
  readVersion,
  recordCollapsed,
  recordExpanded,
  recordMode,
  recordPath,
  recordScroll,
  writeContent,
  writeVersion,
  type DocViewMode
} from './docsCache'
import { chat } from '../../state/chatStore'
import { docs as docsEvents } from '../../state/docsStore'
import { index } from '../../state/indexStore'
import { workspaces } from '../../state/workspaceStore'
import { buildDocTree } from './docsTree'
import type { FileAttachment } from '../chat/textAttachments'
import styles from './DocsScreen.module.css'
import { attachmentFromDocument } from './docAttachment'
import { createDocsEditor, type DocsEditor } from './docsEditor'
import { DocLineMenu } from './DocLineMenu'
import { DocToolbar } from './DocToolbar'
import { useDocAutosave } from './useDocAutosave'
import {
  applyHighlights,
  clearHighlights,
  collectMatches,
  scrollRangeIntoView
} from './findInDocument'
import { useWorkspaceDocs } from './useWorkspaceDocs'
import { AssistantMarkdown } from '../chat/AssistantMarkdown'
import { DocFindBar } from './DocFindBar'
import { DocTree } from './DocTree'

type DocError = { path: string; message: string }

export function DocsScreen(props: {
  workspaceId: string | null
  onAttachLine?: (file: FileAttachment) => void
}) {
  const [docsTick, setDocsTick] = createSignal(0)
  const docs = useWorkspaceDocs(
    () => props.workspaceId,
    () =>
      (chat.activeSessionId ? (chat.reviewTickBySession[chat.activeSessionId] ?? 0) : 0) +
      docsTick()
  )
  const [searchParams, setSearchParams] = useSearchParams<{ file: string }>()
  const urlFile = () => (typeof searchParams.file === 'string' ? searchParams.file : null)
  const [selected, setSelected] = createSignal<string | null>(
    urlFile() ?? lastValidPath(props.workspaceId)
  )
  // A click owns the open document and pushes `file`. The remembered path is
  // written once with replace, so returning to the viewer is not an extra step.
  // Back and forward are the only query changes that move the document: they
  // arrive as `hashchange`. `pushState` does not, so a click is not overwritten
  // by a query write that is still in flight.
  let seeded = false
  createEffect(() => {
    if (seeded) {
      return
    }
    seeded = true
    const current = selected()
    if (current && !urlFile()) {
      setSearchParams({ file: current }, { replace: true })
    }
  })

  onMount(() => {
    const onHashChange = () => {
      const query = window.location.hash.slice(1).split('?')[1] ?? ''
      setSelected(new URLSearchParams(query).get('file'))
    }
    window.addEventListener('hashchange', onHashChange)
    onCleanup(() => window.removeEventListener('hashchange', onHashChange))
  })

  function openDocument(path: string) {
    if (path === selected()) {
      return
    }
    // Leaving the document: flush whatever is pending for the old path.
    flushEdits()
    const workspaceId = props.workspaceId
    setContent(workspaceId ? (readContent(workspaceId, path) ?? null) : null)
    setContentError(null)
    setMergeNotice(null)
    setSelected(path)
    setSearchParams({ file: path })
  }

  const [content, setContent] = createSignal<string | null>(
    props.workspaceId && selected() ? (readContent(props.workspaceId, selected()!) ?? null) : null
  )
  const [contentError, setContentError] = createSignal<DocError | null>(null)

  // Preview or Edit. The choice is remembered per workspace.
  const [mode, setMode] = createSignal<DocViewMode>(
    props.workspaceId ? readMode(props.workspaceId) : 'rendered'
  )
  const [mergeNotice, setMergeNotice] = createSignal<string | null>(null)
  const [editorHost, setEditorHost] = createSignal<HTMLDivElement>()
  const [menuOpen, setMenuOpen] = createSignal(false)
  let editor: DocsEditor | undefined
  let editorPath: string | null = null
  /** A line to reveal once the editor has mounted. See `openInEditor`. */
  let pendingReveal: number | null = null

  const autosave = useDocAutosave({
    workspaceId: () => props.workspaceId,
    path: () => selected(),
    sessionId: () => chat.activeSessionId,
    readText: () => editor?.getText() ?? '',
    onRemoteText: (text) => editor?.replaceAll(text)
  })
  const [lineHover, setLineHover] = createSignal<{
    start: number
    end: number
    top: number
  } | null>(null)
  const [attachNotice, setAttachNotice] = createSignal<string | null>(null)
  const [collapsed, setCollapsed] = createSignal<ReadonlySet<string>>(
    props.workspaceId ? new Set(readCollapsed(props.workspaceId)) : new Set()
  )
  const [opened, setOpened] = createSignal<ReadonlySet<string>>(
    props.workspaceId ? new Set(readExpanded(props.workspaceId)) : new Set()
  )
  const collapsedView = createMemo(() => {
    const next = new Set(collapsed())
    const openedNow = opened()
    for (const entry of docs.entries()) {
      if (
        entry.kind === 'directory' &&
        entry.children_fetched !== true &&
        !openedNow.has(entry.path)
      ) {
        next.add(entry.path)
      }
    }
    return next
  })
  let viewer: HTMLDivElement | undefined
  let sheet: HTMLDivElement | undefined
  let scrollTop = 0
  let findInput: HTMLInputElement | undefined
  let ranges: Range[] = []
  let findKey = ''
  let findQueryNow = ''
  let caseNow = false

  const root = () =>
    workspaces.workspaces.find((workspace) => workspace.id === props.workspaceId)?.root

  createEffect(() => {
    const path = selected()
    const workspaceId = props.workspaceId
    setContent(workspaceId && path ? (readContent(workspaceId, path) ?? null) : null)
    setLineHover(null)
    setAttachNotice(null)
  })

  const [findOpen, setFindOpen] = createSignal(false)
  const [findQuery, setFindQuery] = createSignal('')
  const [caseSensitive, setCaseSensitive] = createSignal(false)
  const [matchCount, setMatchCount] = createSignal(0)
  const [matchIndex, setMatchIndex] = createSignal(-1)
  const [rangeEpoch, setRangeEpoch] = createSignal(0)
  const [draft, setDraft] = createSignal('')
  const [query, setQuery] = createSignal('')
  const [result, setResult] = createSignal<DocSearchResult | null>(null)
  const [searchError, setSearchError] = createSignal<string | null>(null)
  const [searching, setSearching] = createSignal(false)
  const [engine, setEngine] = createSignal<DocSearchEngine>('semantic')
  const [polled, setPolled] = createSignal<IndexStatus | null>(null)
  let debounce: number | null = null
  let abort: AbortController | null = null
  let engineNow: DocSearchEngine = 'semantic'
  let workspaceNow = props.workspaceId
  let queryNow = ''

  createEffect(() => {
    findQueryNow = findQuery()
    caseNow = caseSensitive()
    engineNow = engine()
    workspaceNow = props.workspaceId
    queryNow = query()
  })

  function clearScheduledSearch() {
    if (debounce !== null) {
      window.clearTimeout(debounce)
      debounce = null
    }
  }

  function abortSearch() {
    abort?.abort()
    abort = null
  }

  function beginSearch(q: string) {
    const workspace = workspaceNow
    if (!workspace || !q) {
      return
    }
    abortSearch()
    const controller = new AbortController()
    abort = controller
    queryNow = q
    setQuery(q)
    setSearching(true)
    void searchDocs(workspace, q, undefined, controller.signal, engineNow)
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

  function onDraft(value: string) {
    setDraft(value)
    clearScheduledSearch()
    abortSearch()
    const trimmed = value.trim()
    if (!trimmed) {
      setQuery('')
      queryNow = ''
      setResult(null)
      setPolled(null)
      setSearchError(null)
      setSearching(false)
      return
    }
    setSearchError(null)
    debounce = window.setTimeout(() => {
      debounce = null
      beginSearch(trimmed)
    }, 500)
  }

  function onEngine(next: DocSearchEngine) {
    engineNow = next
    setEngine(next)
    clearScheduledSearch()
    abortSearch()
    const trimmed = draft().trim()
    if (!trimmed) {
      setSearching(false)
      return
    }
    beginSearch(trimmed)
  }

  function refreshSearch() {
    if (debounce !== null) {
      return
    }
    beginSearch(queryNow)
  }

  onCleanup(() => {
    clearScheduledSearch()
    abortSearch()
    clearHighlights()
  })

  createEffect(() => {
    if (props.workspaceId) {
      recordPath(props.workspaceId, selected())
    }
  })

  createEffect(() => {
    if (props.workspaceId) {
      recordCollapsed(props.workspaceId, collapsed())
    }
  })

  createEffect(() => {
    if (props.workspaceId) {
      recordExpanded(props.workspaceId, opened())
    }
  })

  /** Flush the editor's pending save. The flush points beyond the debounce. */
  function flushEdits() {
    void autosave.flush()
  }

  // Mount the editor while Edit is on, and rebuild it when the document
  // changes. Solid runs the component once, so the view is created here rather
  // than in JSX; only a mode or path change tears it down.
  createEffect(() => {
    const host = editorHost()
    const path = selected()
    const text = content()
    const active = mode() === 'edit' && host !== undefined && path !== null && text !== null
    if (!active || editorPath !== path) {
      editor?.destroy()
      editor = undefined
      editorPath = null
    }
    if (!active || !host || path === null || text === null || editor) {
      return
    }
    editor = createDocsEditor({
      parent: host,
      initial: text,
      onChange: (change) => autosave.noteChange(change),
      onSave: () => void autosave.flush()
    })
    editorPath = path
    autosave.begin(readVersion(props.workspaceId ?? '', path) ?? null)
    // "Open in editor" switched the mode; place the cursor once the view exists.
    if (pendingReveal !== null) {
      editor.revealLine(pendingReveal)
      pendingReveal = null
    }
  })

  // The open document refetches on a review tick or a `file_changed` frame. A
  // buffer with unsaved edits is never clobbered; the save merges instead.
  createEffect(() => {
    const workspaceId = props.workspaceId
    const path = selected()
    const reviewTick = chat.activeSessionId
      ? (chat.reviewTickBySession[chat.activeSessionId] ?? 0)
      : 0
    const change = docsEvents.change
    void reviewTick
    void change?.seq
    if (!workspaceId || !path) {
      return
    }
    if (autosave.dirty() && editorPath === path) {
      if (change && change.path === path && change.source === 'agent') {
        setMergeNotice('Agent changed this file — saving will merge.')
      }
      return
    }
    let cancelled = false
    void getDoc(workspaceId, path)
      .then((doc) => {
        if (cancelled || selected() !== path) {
          return
        }
        writeContent(workspaceId, path, doc.content)
        writeVersion(workspaceId, path, doc.version)
        setContent((current) => (current === doc.content ? current : doc.content))
        setContentError(null)
        if (editor && editorPath === path && !autosave.dirty()) {
          if (!editor.hasFocus() && editor.getText() !== doc.content) {
            editor.replaceAll(doc.content)
          }
          autosave.begin(doc.version)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setContentError({
            path,
            message: err instanceof Error ? err.message : 'Failed to load document'
          })
        }
      })
    onCleanup(() => {
      cancelled = true
    })
  })

  // Any file change refreshes the tree; the open document is handled above.
  createEffect(() => {
    if (docsEvents.change) {
      setDocsTick((tick) => tick + 1)
    }
  })

  // A finished save clears the merge notice.
  createEffect(() => {
    if (autosave.status().kind === 'saved') {
      setMergeNotice(null)
    }
  })

  function changeMode(next: DocViewMode) {
    if (next === mode()) {
      return
    }
    flushEdits()
    setMode(next)
    if (props.workspaceId) {
      recordMode(props.workspaceId, next)
    }
    if (next === 'rendered') {
      setMergeNotice(null)
    }
  }

  onMount(() => {
    const onBlur = () => flushEdits()
    window.addEventListener('blur', onBlur)
    onCleanup(() => window.removeEventListener('blur', onBlur))
  })

  onCleanup(() => {
    // The flush reads the buffer synchronously, so destroying after is safe.
    flushEdits()
    editor?.destroy()
    editor = undefined
  })

  createEffect(() => {
    const workspaceId = props.workspaceId
    const path = selected()
    const text = content()
    const node = viewer
    if (!node || !workspaceId || !path || text === null) {
      return
    }
    node.scrollTop = readScroll(workspaceId, path)
    scrollTop = node.scrollTop
  })

  onMount(() => {
    const node = viewer
    const workspaceId = props.workspaceId
    const path = selected()
    if (!node || !workspaceId || !path) {
      return
    }
    const onScroll = () => {
      scrollTop = node.scrollTop
      if (!menuOpen()) {
        setLineHover(null)
      }
    }
    node.addEventListener('scroll', onScroll, { passive: true })
    onCleanup(() => {
      node.removeEventListener('scroll', onScroll)
      recordScroll(workspaceId, path, scrollTop)
    })
  })

  function openFind() {
    setFindOpen(true)
    requestAnimationFrame(() => {
      findInput?.focus()
      findInput?.select()
    })
  }

  function closeFind() {
    setFindOpen(false)
    setFindQuery('')
  }

  function stepMatch(direction: 1 | -1) {
    setMatchIndex((index) => {
      const total = ranges.length
      if (total === 0) {
        return -1
      }
      const start = index < 0 ? 0 : index
      return (start + direction + total) % total
    })
  }

  createEffect(() => {
    const open = findOpen()
    const q = findQuery()
    const sensitive = caseSensitive()
    const text = content()
    const path = selected()
    const node = viewer
    if (!open || !node || text === null) {
      ranges = []
      clearHighlights()
      setMatchCount(0)
      setMatchIndex(-1)
      return
    }
    const next = q ? collectMatches(node, q, sensitive) : []
    ranges = next
    setMatchCount(next.length)
    setRangeEpoch((epoch) => epoch + 1)
    const key = `${path ?? ''}\u0000${sensitive ? 'S' : 'i'}\u0000${q}`
    if (findKey !== key) {
      findKey = key
      setMatchIndex(next.length > 0 ? 0 : -1)
    } else {
      setMatchIndex((index) =>
        next.length === 0 ? -1 : Math.min(Math.max(index, 0), next.length - 1)
      )
    }
  })

  createEffect(() => {
    const open = findOpen()
    const count = matchCount()
    const index = matchIndex()
    rangeEpoch()
    const node = viewer
    if (!open || !node || count === 0) {
      clearHighlights()
      return
    }
    applyHighlights(ranges, index)
    const active = ranges[index]
    if (active) {
      scrollRangeIntoView(node, active)
    }
  })

  createEffect(() => {
    const open = findOpen()
    const path = selected()
    const text = content()
    const onKeyDown = (event: KeyboardEvent) => {
      const isMac = /mac/i.test(navigator.platform || navigator.userAgent)
      const mod = isMac ? event.metaKey : event.ctrlKey
      if (event.key === 'f' || event.key === 'F') {
        if (mod && !event.altKey && path !== null && text !== null) {
          event.preventDefault()
          openFind()
        }
        return
      }
      if (event.key === 'Escape' && open) {
        event.preventDefault()
        closeFind()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })

  createEffect(() => {
    if (!findOpen()) {
      return
    }
    const node = viewer
    if (!node) {
      return
    }
    let frame: number | null = null
    const observer = new MutationObserver(() => {
      if (frame !== null) {
        return
      }
      frame = requestAnimationFrame(() => {
        frame = null
        const next = findQueryNow ? collectMatches(node, findQueryNow, caseNow) : []
        ranges = next
        setMatchCount(next.length)
        setRangeEpoch((epoch) => epoch + 1)
        setMatchIndex((index) =>
          next.length === 0 ? -1 : Math.min(Math.max(index, 0), next.length - 1)
        )
      })
    })
    observer.observe(node, { childList: true, subtree: true, characterData: true })
    onCleanup(() => {
      observer.disconnect()
      if (frame !== null) {
        cancelAnimationFrame(frame)
      }
    })
  })

  const building = createMemo(
    () => result() !== null && result()!.engine !== 'ripgrep' && result()!.index.state !== 'ready'
  )

  createEffect(() => {
    const workspaceId = props.workspaceId
    const q = query()
    const current = result()
    if (!workspaceId || !q || !building() || !current) {
      return
    }
    let cancelled = false
    let seen = current.index.state
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
        .catch(() => {})
    }, 2000)
    onCleanup(() => {
      cancelled = true
      window.clearInterval(timer)
    })
  })

  const tree = createMemo(() => buildDocTree(docs.entries()))
  const selectedError = () => (contentError()?.path === selected() ? contentError()!.message : null)
  const notice = createMemo(() => {
    const found = result()
    if (!found || found.engine === 'ripgrep' || found.index.state === 'ready') {
      return null
    }
    return polled() ?? found.index
  })
  const trimmedDraft = () => draft().trim()
  const searchPending = () => trimmedDraft() !== '' && (searching() || trimmedDraft() !== query())

  function noticeText(status: IndexStatus): string {
    switch (status.state) {
      case 'indexing':
        return status.files_total > 0
          ? `Indexing ${status.files_done}/${status.files_total} — results may be incomplete`
          : 'Preparing search…'
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

  function toggle(path: string) {
    if (collapsedView().has(path)) {
      setCollapsed((current) => {
        const next = new Set(current)
        next.delete(path)
        return next
      })
      setOpened((current) => {
        const next = new Set(current)
        next.add(path)
        return next
      })
      docs.loadDirectory(path)
      return
    }
    setCollapsed((current) => {
      const next = new Set(current)
      next.add(path)
      return next
    })
    setOpened((current) => {
      const next = new Set(current)
      next.delete(path)
      return next
    })
  }

  function onViewerMouseMove(event: MouseEvent) {
    if (menuOpen()) {
      // The menu is in a portal; moving the pointer into it must not retarget
      // the trigger or drop the hovered block.
      return
    }
    const target = event.target
    if (!(target instanceof HTMLElement)) {
      return
    }
    if (target.closest('[data-md-attach]')) {
      return
    }
    const block = target.closest<HTMLElement>('[data-md-lines]')
    const sheetNode = sheet
    const span = block?.dataset.mdLines
    if (!block || !sheetNode || !span) {
      setLineHover(null)
      return
    }
    const [startRaw, endRaw] = span.split('-')
    const start = Number(startRaw)
    const end = Number(endRaw ?? startRaw)
    if (!Number.isFinite(start) || !Number.isFinite(end)) {
      setLineHover(null)
      return
    }
    const top = block.getBoundingClientRect().top - sheetNode.getBoundingClientRect().top
    setLineHover((current) =>
      current && current.start === start && current.end === end && current.top === top
        ? current
        : { start, end, top }
    )
  }

  /** The label for a block's line range, used for the trigger and the menu. */
  function lineRangeLabel(range: { start: number; end: number }): string {
    return range.start === range.end
      ? `line ${range.start}`
      : `lines ${range.start} to ${range.end}`
  }

  function attachLine(range: { start: number; end: number }) {
    setLineHover(null)
    const text = content()
    const path = selected()
    if (!props.onAttachLine || text === null || path === null) {
      return
    }
    try {
      const file = attachmentFromDocument({
        content: text,
        path,
        root: root(),
        startLine: range.start,
        endLine: range.end
      })
      setAttachNotice(null)
      props.onAttachLine(file)
    } catch (error) {
      setAttachNotice(error instanceof Error ? error.message : 'Could not attach the line')
    }
  }

  /**
   * Switch to Edit with the cursor at the end of the block's first source line.
   *
   * The editor does not exist yet when the mode flips, so the line is queued
   * and revealed by the mount effect.
   */
  function openInEditor(range: { start: number; end: number }) {
    setLineHover(null)
    setMenuOpen(false)
    setMergeNotice(null)
    pendingReveal = range.start
    changeMode('edit')
  }

  const hover = lineHover

  return (
    <div class={styles.page}>
      <Show when={docs.entries().length > 0}>
        <header class={styles.header}>
          <div class={styles.searchWrap}>
            <input
              class={styles.search}
              type="search"
              value={draft()}
              placeholder="Search documentation…"
              aria-label="Search documentation"
              aria-busy={searchPending() || undefined}
              onInput={(event) => onDraft(event.currentTarget.value)}
            />
            <Show when={searchPending()}>
              <span class={styles.spinner} aria-hidden="true" />
            </Show>
          </div>
          <div class={styles.engines} role="group" aria-label="Search engine">
            <button
              type="button"
              class={styles.engine}
              aria-pressed={engine() === 'semantic'}
              onClick={() => onEngine('semantic')}
            >
              Semantic
            </button>
            <button
              type="button"
              class={styles.engine}
              aria-pressed={engine() === 'ripgrep'}
              onClick={() => onEngine('ripgrep')}
            >
              Text
            </button>
          </div>
        </header>
      </Show>
      <Show when={props.workspaceId} fallback={<p class={styles.message}>No workspace is open.</p>}>
        <Show
          when={!docs.error()}
          fallback={
            <p class={styles.message} role="alert">
              {docs.error()}
            </p>
          }
        >
          <Show when={!docs.loading()} fallback={<p class={styles.message}>Loading docs…</p>}>
            <Show
              when={docs.entries().length > 0}
              fallback={<p class={styles.message}>No markdown files in this workspace.</p>}
            >
              <div class={styles.body}>
                <Show
                  when={trimmedDraft()}
                  fallback={
                    <DocTree
                      nodes={tree()}
                      selected={selected()}
                      collapsed={collapsedView()}
                      onSelect={openDocument}
                      onToggle={toggle}
                    />
                  }
                >
                  <div class={styles.results}>
                    <Show when={notice()}>
                      {(status) => (
                        <p class={styles.notice} role="status">
                          {noticeText(status())}
                          <Show when={status().state === 'paused' || status().state === 'failed'}>
                            <button
                              type="button"
                              class={styles.resume}
                              onClick={() => {
                                void index.setPaused(false).then(() => refreshSearch())
                              }}
                            >
                              Resume
                            </button>
                          </Show>
                        </p>
                      )}
                    </Show>
                    <Show
                      when={!searchError()}
                      fallback={
                        <p class={styles.hint} role="alert">
                          {searchError()}
                        </p>
                      }
                    >
                      <Show
                        when={
                          !(searchPending() && (!result() || result()!.query !== trimmedDraft()))
                        }
                        fallback={
                          <p class={styles.hint} role="status">
                            <span class={styles.spinner} aria-hidden="true" />
                            Searching…
                          </p>
                        }
                      >
                        <Show
                          when={result() && result()!.hits.length > 0}
                          fallback={<p class={styles.hint}>No matches for “{trimmedDraft()}”.</p>}
                        >
                          <ul class={styles.hits}>
                            <For each={result()?.hits ?? []}>
                              {(hit) => (
                                <li>
                                  <button
                                    type="button"
                                    class={styles.hit}
                                    onClick={() => openDocument(hit.path)}
                                  >
                                    <span class={styles.hitTitle}>
                                      {hit.title || hit.path.split('/').pop()}
                                    </span>
                                    <span class={styles.hitPath}>{hit.path}</span>
                                    <span class={styles.hitSnippet}>{hit.snippet}</span>
                                  </button>
                                </li>
                              )}
                            </For>
                          </ul>
                        </Show>
                      </Show>
                    </Show>
                  </div>
                </Show>
                <div class={styles.viewerWrap}>
                  <Show when={selected() !== null && content() !== null}>
                    <DocToolbar
                      mode={mode()}
                      onMode={changeMode}
                      onSave={flushEdits}
                      dirty={autosave.dirty()}
                      status={autosave.status()}
                      notice={mergeNotice()}
                    />
                  </Show>
                  <Show
                    when={
                      findOpen() && !selectedError() && selected() !== null && content() !== null
                    }
                  >
                    <DocFindBar
                      query={findQuery()}
                      onQuery={setFindQuery}
                      count={matchCount()}
                      current={matchIndex()}
                      caseSensitive={caseSensitive()}
                      onCaseSensitive={setCaseSensitive}
                      onNext={() => stepMatch(1)}
                      onPrevious={() => stepMatch(-1)}
                      onClose={closeFind}
                      input={(el) => {
                        findInput = el
                      }}
                    />
                  </Show>
                  <Show when={attachNotice()}>
                    <p class={styles.attachNotice} role="alert">
                      {attachNotice()}
                    </p>
                  </Show>
                  <div
                    class={styles.viewer}
                    ref={viewer}
                    onMouseMove={onViewerMouseMove}
                    onMouseLeave={() => {
                      if (!menuOpen()) {
                        setLineHover(null)
                      }
                    }}
                  >
                    <Show
                      when={!selectedError()}
                      fallback={
                        <p class={styles.message} role="alert">
                          {selectedError()}
                        </p>
                      }
                    >
                      <Show
                        when={selected() !== null && content() !== null ? selected()! : null}
                        keyed
                        fallback={
                          <div class={styles.placeholder}>Select a document to open it.</div>
                        }
                      >
                        {(path) => (
                          <Show
                            when={mode() === 'edit'}
                            fallback={
                              <div class={styles.sheet} ref={sheet}>
                                <AssistantMarkdown
                                  text={content() ?? ''}
                                  document
                                  docPath={path}
                                  onDocLink={openDocument}
                                />
                                <Show when={hover()}>
                                  <DocLineMenu
                                    top={hover()!.top}
                                    label={`Line actions for ${lineRangeLabel(hover()!)}`}
                                    onAddToChat={
                                      props.onAttachLine
                                        ? () => {
                                            const range = hover()
                                            if (range) {
                                              attachLine(range)
                                            }
                                          }
                                        : undefined
                                    }
                                    onOpenInEditor={() => {
                                      const range = hover()
                                      if (range) {
                                        openInEditor(range)
                                      }
                                    }}
                                    onOpenChange={setMenuOpen}
                                  />
                                </Show>
                              </div>
                            }
                          >
                            <div class={styles.editor} ref={(el) => setEditorHost(el)} />
                          </Show>
                        )}
                      </Show>
                    </Show>
                  </div>
                </div>
              </div>
            </Show>
          </Show>
        </Show>
      </Show>
    </div>
  )
}
