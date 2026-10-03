import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { Composer } from './Composer'
import styles from './Composer.module.css'
import type { AgentMode } from '../../api/sessions'

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

  it('marks ask and plan with their colors', () => {
    renderComposer('ask')
    expect(screen.getByRole('button', { name: 'Mode' }).className).toContain(styles.modeAsk)
    cleanup()
    renderComposer('plan')
    expect(screen.getByRole('button', { name: 'Mode' }).className).toContain(styles.modePlan)
  })
})
