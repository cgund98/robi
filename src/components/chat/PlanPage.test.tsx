import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { PlanPage } from './PlanPage'
import type { PlanView } from './toolCallView'

const plan: PlanView = {
  title: 'Ship modes',
  summary: 'Add ask, plan, and agent.',
  body: '# Ship modes\n\nAdd ask, plan, and agent.\n',
  path: '.robi/plans/ship-modes.md',
  created: true,
  todos: [
    { id: 'modes', content: 'Add the mode registry', status: 'completed' },
    { id: 'wire', content: 'Register the tool', status: 'in_progress' },
    { id: 'prompt', content: 'Re-read the list', status: 'pending' },
    { id: 'drop', content: 'Skip the panel', status: 'canceled' }
  ]
}

describe('PlanPage', () => {
  afterEach(() => {
    cleanup()
  })

  it('fills the page with the plan and its steps', () => {
    const onBack = vi.fn()
    const onBuild = vi.fn()
    render(<PlanPage plan={plan} buildDisabled={false} onBack={onBack} onBuild={onBuild} />)

    expect(
      within(screen.getByRole('banner')).getByRole('heading', { name: 'Ship modes' })
    ).toBeTruthy()
    expect(screen.getByText('Add ask, plan, and agent.')).toBeTruthy()
    expect(screen.getByText('Add the mode registry')).toBeTruthy()
    expect(screen.getByText('Register the tool').closest('li')?.getAttribute('data-status')).toBe(
      'in_progress'
    )
    expect(screen.getByText('Re-read the list').closest('li')?.getAttribute('data-status')).toBe(
      'pending'
    )
    expect(screen.getByText('Skip the panel').closest('li')?.getAttribute('data-status')).toBe(
      'canceled'
    )

    fireEvent.click(screen.getByRole('button', { name: '← Back' }))
    expect(onBack).toHaveBeenCalledOnce()
    fireEvent.click(screen.getByRole('button', { name: 'Build' }))
    expect(onBuild).toHaveBeenCalledOnce()
  })

  it('disables Build while the session is busy', () => {
    render(<PlanPage plan={plan} buildDisabled onBack={() => {}} onBuild={() => {}} />)
    expect(screen.getByRole('button', { name: 'Build' }).hasAttribute('disabled')).toBe(true)
  })
})
