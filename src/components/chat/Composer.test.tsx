import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { Composer } from './Composer'
import styles from './Composer.module.css'
import type { AgentMode } from '../../api/sessions'
import { claimComposerDraft, writeComposerDraft } from '../../state/composerDrafts'

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

  it('marks ask and plan with their colors', () => {
    renderComposer('ask')
    expect(screen.getByRole('button', { name: 'Mode' }).className).toContain(styles.modeAsk)
    cleanup()
    renderComposer('plan')
    expect(screen.getByRole('button', { name: 'Mode' }).className).toContain(styles.modePlan)
  })
})
