import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ContextMeter } from './ContextMeter'

afterEach(cleanup)

const messages = [
  {
    content: 'hello',
    tool_calls: [],
    usage: { input: 1000, output: 20, cached: 0 }
  }
]

describe('ContextMeter Compact action', () => {
  it('runs the compact handler when clicked', () => {
    const onCompact = vi.fn()
    render(
      <ContextMeter
        messages={messages}
        draft=""
        pendingText=""
        contextWindow={100_000}
        onCompact={onCompact}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /Context/ }))
    const compact = screen.getByRole('button', { name: 'Compact' })
    fireEvent.click(compact)
    expect(onCompact).toHaveBeenCalledTimes(1)
  })

  it('disables Compact while a compact or a turn is in flight', () => {
    const { rerender } = render(
      <ContextMeter
        messages={messages}
        draft=""
        pendingText=""
        contextWindow={100_000}
        onCompact={() => {}}
        compacting
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /Context/ }))
    expect(
      (screen.getByRole('button', { name: 'Compacting…' }) as HTMLButtonElement).disabled
    ).toBe(true)

    rerender(
      <ContextMeter
        messages={messages}
        draft=""
        pendingText=""
        contextWindow={100_000}
        onCompact={() => {}}
        compactDisabled
      />
    )
    expect((screen.getByRole('button', { name: 'Compact' }) as HTMLButtonElement).disabled).toBe(
      true
    )
  })

  it('shows no Compact button when there is no handler', () => {
    render(<ContextMeter messages={messages} draft="" pendingText="" contextWindow={100_000} />)
    fireEvent.click(screen.getByRole('button', { name: /Context/ }))
    expect(screen.queryByRole('button', { name: 'Compact' })).toBeNull()
  })
})
