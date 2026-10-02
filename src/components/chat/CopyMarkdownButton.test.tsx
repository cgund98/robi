import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { CopyMarkdownButton } from './CopyMarkdownButton'

describe('CopyMarkdownButton', () => {
  it('copies the markdown text as stored', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    vi.stubGlobal('navigator', { clipboard: { writeText } })
    render(<CopyMarkdownButton text={'## Title\n\n- item'} />)
    fireEvent.click(screen.getByRole('button', { name: 'Copy markdown' }))
    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith('## Title\n\n- item')
      expect(screen.getByRole('tooltip', { name: 'Copied' })).toBeTruthy()
    })
    vi.unstubAllGlobals()
  })
})
