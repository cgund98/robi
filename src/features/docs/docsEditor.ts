/**
 * CodeMirror 6 adapter for the docs viewer.
 *
 * One `EditorView`, built once against a ref. The component never recreates it
 * on render; remote text arrives through `replaceAll`, which does not report
 * itself as a user change.
 *
 * The editor reports each change as a `ChangeSet`. The autosave hook owns the
 * base version and the accumulation; this file owns only the buffer.
 */
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { markdown } from '@codemirror/lang-markdown'
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { ChangeSet, EditorState, Transaction } from '@codemirror/state'
import { EditorView, drawSelection, keymap, type KeyBinding } from '@codemirror/view'
import { tags } from '@lezer/highlight'

/** One wire range: replace `from..to` (UTF-16 units) with `insert`. */
export type ChangeRange = { from: number; to: number; insert: string }

export type DocsEditor = {
  /** The whole buffer. */
  getText: () => string
  /** Replace the buffer without reporting a user change. Remote adoption. */
  replaceAll: (text: string) => void
  /**
   * Put the cursor at the end of a 1-based line and focus the editor. A line
   * outside the document clamps to the first or the last. The move is not an
   * undo step.
   */
  revealLine: (line: number) => void
  /** The cursor's character offset. */
  cursor: () => number
  focus: () => void
  hasFocus: () => boolean
  destroy: () => void
}

export function createDocsEditor(options: {
  parent: HTMLElement
  initial: string
  /** Every user change, in order. A remote `replaceAll` does not fire it. */
  onChange: (change: ChangeSet) => void
  /** Mod-s. The screen flushes the debounce immediately. */
  onSave: () => void
}): DocsEditor {
  let applyingRemote = false

  const saveKey: KeyBinding = {
    key: 'Mod-s',
    preventDefault: true,
    run: () => {
      options.onSave()
      return true
    }
  }

  const state = EditorState.create({
    doc: options.initial,
    extensions: [
      history(),
      markdown(),
      // Draw the cursor rather than leaning on the browser's native caret. The
      // production build runs in WebKit, which does not render the native caret
      // in CodeMirror's contenteditable reliably. The theme styles `.cm-cursor`.
      drawSelection(),
      EditorView.lineWrapping,
      keymap.of([saveKey, ...defaultKeymap, ...historyKeymap]),
      syntaxHighlighting(markdownHighlight),
      theme,
      EditorView.updateListener.of((update) => {
        if (!update.docChanged || applyingRemote) {
          return
        }
        options.onChange(update.changes)
      })
    ]
  })

  const view = new EditorView({ state, parent: options.parent })

  return {
    getText: () => view.state.doc.toString(),
    replaceAll: (text) => {
      applyingRemote = true
      try {
        view.dispatch({
          changes: { from: 0, to: view.state.doc.length, insert: text },
          annotations: Transaction.addToHistory.of(false)
        })
      } finally {
        applyingRemote = false
      }
    },
    focus: () => view.focus(),
    hasFocus: () => view.hasFocus,
    revealLine: (line) => {
      const doc = view.state.doc
      const target = Math.min(Math.max(Math.trunc(line), 1), doc.lines)
      view.dispatch({
        selection: { anchor: doc.line(target).to },
        scrollIntoView: true,
        annotations: Transaction.addToHistory.of(false)
      })
      view.focus()
    },
    cursor: () => view.state.selection.main.head,
    destroy: () => view.destroy()
  }
}

/**
 * Flatten a `ChangeSet` into wire ranges, in the base document's coordinates.
 * `iterChanges` reports `fromA`/`toA` against the base, which is what the
 * server's `apply_changes` expects.
 */
export function serializeChangeSet(set: ChangeSet): ChangeRange[] {
  const ranges: ChangeRange[] = []
  set.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
    ranges.push({ from: fromA, to: toA, insert: inserted.toString() })
  })
  return ranges
}

/**
 * Markdown syntax colors, from the same tokens as the rendered view.
 *
 * `@codemirror/lang-markdown` tags headings, emphasis, links, inline code,
 * quotes, rules, and list markers; the generic code tags below are ready for a
 * fenced language once one is registered.
 */
const markdownHighlight = HighlightStyle.define([
  { tag: tags.heading1, color: 'var(--ink-bright)', fontWeight: '700', fontSize: '1.5em' },
  { tag: tags.heading2, color: 'var(--ink-bright)', fontWeight: '700', fontSize: '1.3em' },
  { tag: tags.heading3, color: 'var(--ink-bright)', fontWeight: '600', fontSize: '1.15em' },
  {
    tag: [tags.heading4, tags.heading5, tags.heading6],
    color: 'var(--ink-strong)',
    fontWeight: '600'
  },
  { tag: tags.heading, color: 'var(--ink-bright)', fontWeight: '600' },
  { tag: tags.strong, color: 'var(--ink-bright)', fontWeight: '700' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strikethrough, color: 'var(--ink-muted)', textDecoration: 'line-through' },
  { tag: [tags.link, tags.url], color: 'var(--accent)', textDecoration: 'underline' },
  { tag: tags.monospace, color: 'var(--accent)' },
  { tag: tags.quote, color: 'var(--ink-muted)', fontStyle: 'italic' },
  { tag: tags.contentSeparator, color: 'var(--ink-faint)' },
  { tag: [tags.processingInstruction, tags.punctuation], color: 'var(--ink-faint)' },
  { tag: [tags.keyword, tags.operator, tags.tagName, tags.typeName], color: 'var(--accent)' },
  { tag: [tags.string, tags.special(tags.string), tags.regexp], color: 'var(--success)' },
  {
    tag: [tags.comment, tags.lineComment, tags.blockComment],
    color: 'var(--ink-faint)',
    fontStyle: 'italic'
  },
  { tag: [tags.number, tags.bool, tags.null], color: 'var(--warn)' },
  { tag: tags.attributeName, color: 'var(--mode-plan)' },
  { tag: [tags.variableName, tags.propertyName], color: 'var(--ink-strong)' },
  { tag: tags.invalid, color: 'var(--danger)' }
])

/** The editor theme, built from the shell's tokens. */
const theme = EditorView.theme(
  {
    '&': {
      color: 'var(--ink)',
      backgroundColor: 'var(--bg-canvas)',
      fontFamily: 'var(--font-mono)',
      fontSize: '13px',
      height: '100%'
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-scroller': {
      fontFamily: 'inherit',
      lineHeight: '1.6',
      overflow: 'auto'
    },
    '.cm-content': {
      caretColor: 'var(--accent)',
      padding: '16px 0'
    },
    '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--accent)' },
    '.cm-gutters': {
      backgroundColor: 'var(--bg-canvas)',
      color: 'var(--ink-faint)',
      border: 'none'
    },
    '.cm-activeLine': { backgroundColor: 'var(--bg-surface)' },
    '.cm-activeLineGutter': {
      backgroundColor: 'transparent',
      color: 'var(--ink-muted)'
    },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': {
      backgroundColor: 'var(--bg-surface-active)'
    },
    '.cm-selectionMatch': { backgroundColor: 'var(--find-hit)' },
    '.cm-panels': {
      backgroundColor: 'var(--bg-surface)',
      color: 'var(--ink)'
    }
  },
  { dark: true }
)
