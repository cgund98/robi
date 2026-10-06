import { describe, expect, it } from 'vitest'

import { createDocsEditor } from './docsEditor'

function mount(initial: string, onChange: () => void = () => {}) {
  const parent = document.createElement('div')
  document.body.appendChild(parent)
  const editor = createDocsEditor({ parent, initial, onChange, onSave: () => {} })
  return {
    editor,
    parent,
    dispose: () => {
      editor.destroy()
      parent.remove()
    }
  }
}

/** Class names on the spans CodeMirror emitted inside the document. */
function tokenClasses(parent: HTMLElement): string[] {
  return [...parent.querySelectorAll('.cm-content span')].map((el) => el.className)
}

describe('docsEditor', () => {
  it('syntax-highlights markdown rather than rendering plain text', () => {
    // A heading, bold, a link, and inline code — four different tag groups.
    const { parent, dispose } = mount('# Title\n\nSome **bold** and `code` and [a](b).\n')
    const classes = tokenClasses(parent)
    expect(classes.length).toBeGreaterThan(0)
    expect(classes.some((name) => name.trim().length > 0)).toBe(true)
    dispose()
  })

  it('replaceAll does not report a user change', () => {
    let changes = 0
    const { editor, dispose } = mount('one\n', () => {
      changes += 1
    })
    editor.replaceAll('two\n')
    expect(editor.getText()).toBe('two\n')
    expect(changes).toBe(0)
    dispose()
  })

  it('getText reflects the buffer', () => {
    const { editor, dispose } = mount('hello\n')
    expect(editor.getText()).toBe('hello\n')
    dispose()
  })

  it('draws the cursor itself rather than relying on the native caret', () => {
    // WebKit, the production webview, does not render CodeMirror's native caret
    // reliably, so the editor must register `drawSelection` and draw its own
    // `.cm-cursor`/`.cm-selectionLayer` elements. The layer is absent otherwise.
    const { editor, parent, dispose } = mount('hello\n')
    editor.focus()
    expect(parent.querySelector('.cm-cursorLayer')).not.toBeNull()
    expect(parent.querySelector('.cm-selectionLayer')).not.toBeNull()
    dispose()
  })

  it('revealLine puts the cursor at the end of the line', () => {
    // One line per entry: 0..3, 4..7, 8..13.
    const { editor, dispose } = mount('one\ntwo\nthree\n')
    editor.revealLine(2)
    expect(editor.cursor()).toBe(7)
    editor.revealLine(1)
    expect(editor.cursor()).toBe(3)
    editor.revealLine(3)
    expect(editor.cursor()).toBe(13)
    dispose()
  })

  it('revealLine clamps a line outside the document', () => {
    // "one\ntwo\n" has three lines: "one", "two", and the empty line after the
    // trailing newline. The last line ends at the buffer length, 8.
    const { editor, dispose } = mount('one\ntwo\n')
    editor.revealLine(99)
    expect(editor.cursor()).toBe(8)
    editor.revealLine(0)
    expect(editor.cursor()).toBe(3)
    dispose()
  })
})
