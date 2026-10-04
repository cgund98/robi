import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { DeleteSessionDialog } from './DeleteSessionDialog'

function renderDialog(overrides: Partial<Parameters<typeof DeleteSessionDialog>[0]> = {}) {
  const onCancel = vi.fn()
  const onDelete = vi.fn()
  render(
    <DeleteSessionDialog
      open
      title="Parser cleanup"
      running={false}
      busy={false}
      error={null}
      onCancel={onCancel}
      onDelete={onDelete}
      {...overrides}
    />
  )
  return { onCancel, onDelete }
}

describe('DeleteSessionDialog', () => {
  afterEach(() => {
    cleanup()
  })

  it('names the session and confirms on Delete', () => {
    const { onDelete } = renderDialog()

    expect(screen.getByText(/Parser cleanup/)).toBeTruthy()
    expect(screen.getByText(/This cannot be undone/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    expect(onDelete).toHaveBeenCalledOnce()
  })

  it('cancels without deleting', () => {
    const { onCancel, onDelete } = renderDialog()

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onCancel).toHaveBeenCalledOnce()
    expect(onDelete).not.toHaveBeenCalled()
  })

  it('shows the running line only while the agent is running', () => {
    renderDialog({ running: true })
    expect(screen.getByText('A running turn will be stopped.')).toBeTruthy()
  })

  it('locks both buttons and spins the Delete button while busy', () => {
    renderDialog({ busy: true })

    const del = screen.getByRole('button', { name: 'Delete' })
    expect(del.hasAttribute('disabled')).toBe(true)
    expect(del.querySelector('span')).not.toBeNull()
    expect(screen.getByRole('button', { name: 'Cancel' }).hasAttribute('disabled')).toBe(true)
  })

  it('renders the error in an alert', () => {
    renderDialog({ error: 'Failed to delete chat session' })
    expect(screen.getByRole('alert').textContent).toBe('Failed to delete chat session')
  })
})
