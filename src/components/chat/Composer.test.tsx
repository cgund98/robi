import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { Composer } from './Composer'
import styles from './Composer.module.css'
import { MAX_ATTACHMENTS } from './textAttachments'
import type { AgentMode } from '../../api/sessions'
import { claimComposerDraft, writeComposerDraft } from '../../state/composerDrafts'
import { requestComposerAttachment } from '../../state/composerAttachments'

function renderComposer(mode: AgentMode) {
  render(
    <Composer
      disabled={false}
      onSubmit={async () => true}
      models={[]}
      mode={mode}
      modelId={null}
      effort={null}
      defaultModelId="model"
      defaultEffort={null}
      onModeChange={() => {}}
      onModelChange={() => {}}
      onEffortChange={() => {}}
      messages={[]}
      draftKey="session-a"
    />
  )
}

describe('Composer mode color', () => {
  afterEach(() => {
    cleanup()
  })

  it('keeps agent gray', () => {
    renderComposer('agent')
    const mode = screen.getByRole('button', { name: 'Mode' })
    expect(mode.className).not.toContain(styles.modeAsk)
    expect(mode.className).not.toContain(styles.modePlan)
  })

  it('keeps each session draft when the composer remounts', () => {
    const props = {
      disabled: false,
      onSubmit: async () => true,
      models: [],
      mode: 'agent' as const,
      modelId: null,
      effort: null,
      defaultModelId: 'model',
      defaultEffort: null,
      onModeChange: () => {},
      onModelChange: () => {},
      onEffortChange: () => {},
      messages: []
    }
    const first = render(<Composer {...props} draftKey="session-a" />)
    fireEvent.change(screen.getByRole('textbox', { name: 'Message' }), {
      target: { value: 'keep this' }
    })
    first.unmount()

    render(<Composer {...props} draftKey="session-b" />)
    expect((screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement).value).toBe('')
    cleanup()

    render(<Composer {...props} draftKey="session-a" />)
    expect((screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement).value).toBe(
      'keep this'
    )
  })

  it('clears the field after a successful send', async () => {
    renderComposer('agent')
    const field = screen.getByRole('textbox', { name: 'Message' })
    fireEvent.change(field, { target: { value: 'hello' } })
    await act(async () => {
      fireEvent.keyDown(field, { key: 'Enter' })
    })
    expect((screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement).value).toBe('')
  })

  it('clears after the first send moves the draft onto a session', async () => {
    const props = {
      disabled: false,
      models: [],
      mode: 'agent' as const,
      modelId: null,
      effort: null,
      defaultModelId: 'model',
      defaultEffort: null,
      onModeChange: () => {},
      onModelChange: () => {},
      onEffortChange: () => {},
      messages: []
    }
    const view = render(
      <Composer
        {...props}
        draftKey="draft"
        onSubmit={async (text) => {
          claimComposerDraft('session-new', text)
          view.rerender(<Composer {...props} draftKey="session-new" onSubmit={async () => true} />)
          writeComposerDraft('session-new', '')
          return true
        }}
      />
    )
    const field = screen.getByRole('textbox', { name: 'Message' })
    fireEvent.change(field, { target: { value: 'first message' } })
    await act(async () => {
      fireEvent.keyDown(field, { key: 'Enter' })
    })
    expect((screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement).value).toBe('')
  })

  it('adds a pasted image to the field', () => {
    renderComposer('agent')
    const field = screen.getByRole('textbox', { name: 'Message' })
    fireEvent.paste(field, {
      clipboardData: {
        files: [new File(['x'], 'shot.png', { type: 'image/png' })],
        items: []
      }
    })
    expect(screen.getByRole('button', { name: 'Remove shot.png' })).toBeTruthy()
  })

  it('adds a dropped text file to the field', async () => {
    renderComposer('agent')
    const field = screen.getByRole('textbox', { name: 'Message' })
    await act(async () => {
      fireEvent.drop(field, {
        dataTransfer: {
          types: ['Files'],
          files: [new File(['fn main() {}'], 'main.rs', { type: 'text/plain' })],
          items: []
        }
      })
    })
    expect(screen.getByRole('button', { name: 'Remove main.rs' })).toBeTruthy()
  })

  it('adds a text file through the paperclip and removes it as a chip', async () => {
    renderComposer('agent')
    const input = screen.getByLabelText('Attach files') as HTMLInputElement
    await act(async () => {
      fireEvent.change(input, {
        target: { files: [new File(['fn main() {}'], 'main.rs', { type: 'text/plain' })] }
      })
    })
    const remove = screen.getByRole('button', { name: 'Remove main.rs' })
    fireEvent.click(remove)
    expect(screen.queryByRole('button', { name: 'Remove main.rs' })).toBeNull()
  })

  it('sends the attachments along with the draft', async () => {
    const seen: { text: string; files?: { name: string }[] }[] = []
    render(
      <Composer
        disabled={false}
        onSubmit={async (text, _images, files) => {
          seen.push({ text, files })
          return true
        }}
        models={[]}
        mode="agent"
        modelId={null}
        effort={null}
        defaultModelId="model"
        defaultEffort={null}
        onModeChange={() => {}}
        onModelChange={() => {}}
        onEffortChange={() => {}}
        messages={[]}
        draftKey="session-a"
      />
    )
    const field = screen.getByRole('textbox', { name: 'Message' })
    const input = screen.getByLabelText('Attach files') as HTMLInputElement
    await act(async () => {
      fireEvent.change(input, {
        target: { files: [new File(['fn main() {}'], 'main.rs', { type: 'text/plain' })] }
      })
    })
    fireEvent.change(field, { target: { value: 'look' } })
    await act(async () => {
      fireEvent.keyDown(field, { key: 'Enter' })
    })
    expect(seen).toHaveLength(1)
    expect(seen[0].text).toBe('look')
    expect(seen[0].files?.[0].name).toBe('main.rs')
  })

  it('drains an attachment requested for the current draft key', async () => {
    renderComposer('agent')
    await act(async () => {
      requestComposerAttachment('session-a', {
        name: 'guide.md',
        contentBase64: '',
        size: 0,
        startLine: 3,
        endLine: 4
      })
    })
    expect(screen.getByRole('button', { name: 'Remove guide.md' })).toBeTruthy()
  })

  it('holds requests for another key until that composer is shown', async () => {
    renderComposer('agent')
    await act(async () => {
      requestComposerAttachment('session-b', { name: 'other.md', contentBase64: '', size: 0 })
    })
    expect(screen.queryByRole('button', { name: 'Remove other.md' })).toBeNull()
  })

  it('drops requests past the attachment cap and explains why', async () => {
    renderComposer('agent')
    await act(async () => {
      for (let index = 0; index < MAX_ATTACHMENTS + 1; index += 1) {
        requestComposerAttachment('session-a', {
          name: `f${index}.md`,
          contentBase64: '',
          size: 0
        })
      }
    })
    expect(screen.getAllByRole('button', { name: /^Remove / })).toHaveLength(MAX_ATTACHMENTS)
    expect(screen.getByRole('alert').textContent).toContain('up to')
  })

  it('marks ask and plan with their colors', () => {
    renderComposer('ask')
    expect(screen.getByRole('button', { name: 'Mode' }).className).toContain(styles.modeAsk)
    cleanup()
    renderComposer('plan')
    expect(screen.getByRole('button', { name: 'Mode' }).className).toContain(styles.modePlan)
  })
})
