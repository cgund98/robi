import { render } from 'solid-js/web'
import { afterEach, describe, expect, it } from 'vitest'

import { Composer } from './Composer'
import { readComposerDraft } from '../../state/composerDrafts'
import { requestComposerAttachment } from '../../state/composerAttachments'
import type { FileAttachment } from './textAttachments'

let dispose: (() => void) | undefined

afterEach(() => {
  dispose?.()
  dispose = undefined
  document.body.textContent = ''
})

function mount(draftKey: string, attachment?: FileAttachment) {
  if (attachment) {
    requestComposerAttachment(draftKey, attachment)
  }
  const container = document.createElement('div')
  document.body.appendChild(container)
  dispose = render(
    () => (
      <Composer
        disabled={false}
        onSubmit={async () => true}
        models={[]}
        mode="agent"
        modelId={null}
        effort={null}
        defaultModelId="stub-model"
        defaultEffort={null}
        onModeChange={() => {}}
        onModelChange={() => {}}
        onEffortChange={() => {}}
        messages={[]}
        draftKey={draftKey}
      />
    ),
    container
  )
  const field = () => {
    const textarea = container.querySelector('textarea')
    if (!(textarea instanceof HTMLTextAreaElement)) {
      throw new Error('no textarea')
    }
    return textarea
  }
  const type = (value: string) => {
    const textarea = field()
    textarea.value = value
    textarea.dispatchEvent(new Event('input', { bubbles: true }))
  }
  return { container, field, type }
}

function chord(): KeyboardEvent {
  return new KeyboardEvent('keydown', { key: 'k', metaKey: true, shiftKey: true, bubbles: true })
}

// Kobalte keeps the content mounted after a close and marks it `data-closed`,
// so an open dialog is the one without the attribute.
function dialog(): Element | null {
  return document.querySelector('[role="dialog"]:not([data-closed])')
}

function button(label: string): HTMLButtonElement {
  const match = [...document.querySelectorAll('[role="dialog"] button')].find(
    (node) => node.textContent?.trim() === label
  )
  if (!(match instanceof HTMLButtonElement)) {
    throw new Error(`no ${label} button`)
  }
  return match
}

const attachment: FileAttachment = {
  name: 'notes.md',
  path: 'docs/notes.md',
  contentBase64: 'aGVsbG8=',
  size: 5
}

describe('Composer clear shortcut', () => {
  it('asks before clearing and leaves the text on cancel', () => {
    const { type } = mount('clear-cancel')
    type('draft text')
    expect(readComposerDraft('clear-cancel')).toBe('draft text')

    window.dispatchEvent(chord())
    expect(dialog()?.textContent).toContain('Clear composer')
    expect(dialog()?.textContent).toContain('The text will be removed.')

    button('Cancel').click()
    expect(dialog()).toBeNull()
    expect(readComposerDraft('clear-cancel')).toBe('draft text')
  })

  it('empties the field, the draft, and the attachments on clear', () => {
    const { container, field, type } = mount('clear-confirm', attachment)
    type('draft text')
    expect(container.textContent).toContain('notes.md')

    window.dispatchEvent(chord())
    expect(dialog()?.textContent).toContain('The text and 1 attachment will be removed.')

    button('Clear').click()
    expect(dialog()).toBeNull()
    expect(field().value).toBe('')
    expect(readComposerDraft('clear-confirm')).toBe('')
    expect(container.textContent).not.toContain('notes.md')
  })

  it('opens nothing when the composer is empty', () => {
    mount('clear-empty')
    window.dispatchEvent(chord())
    expect(dialog()).toBeNull()
  })

  it('leaves the chord to an open dialog', () => {
    const { type } = mount('clear-guard')
    type('draft text')
    window.dispatchEvent(chord())
    const content = dialog()
    expect(content).not.toBeNull()

    const inside = chord()
    content?.dispatchEvent(inside)
    expect(inside.defaultPrevented).toBe(false)
    expect(document.querySelectorAll('[role="dialog"]:not([data-closed])')).toHaveLength(1)
  })

  it('ignores the chord without shift and a held key', () => {
    const { type } = mount('clear-modifiers')
    type('draft text')

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', metaKey: true, bubbles: true }))
    expect(dialog()).toBeNull()

    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'k', metaKey: true, shiftKey: true, repeat: true })
    )
    expect(dialog()).toBeNull()
  })
})
