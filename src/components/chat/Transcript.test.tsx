import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Transcript } from './Transcript'

describe('Transcript activity dots', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('steps the dots beside Thinking', () => {
    vi.useFakeTimers()
    render(
      <Transcript messages={[]} echo={null} phase="thinking" deciding={false} onDecide={() => {}} />
    )
    const dots = document.querySelector('[aria-hidden]')
    expect(screen.getByText('Thinking')).toBeTruthy()
    expect(dots?.textContent).toBe('.')
    act(() => {
      vi.advanceTimersByTime(400)
    })
    expect(dots?.textContent).toBe('..')
    act(() => {
      vi.advanceTimersByTime(400)
    })
    expect(dots?.textContent).toBe('...')
    act(() => {
      vi.advanceTimersByTime(400)
    })
    expect(dots?.textContent).toBe('.')
  })
})
