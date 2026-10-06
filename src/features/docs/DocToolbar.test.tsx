/** @jsxImportSource solid-js */
import { render } from 'solid-js/web'
import { describe, expect, it } from 'vitest'

import { DocToolbar } from './DocToolbar'

function mount() {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const dispose = render(
    () => (
      <DocToolbar
        mode="rendered"
        onMode={() => {}}
        onSave={() => {}}
        dirty={false}
        status={{ kind: 'idle' }}
        notice={null}
      />
    ),
    container
  )
  return { container, dispose }
}

describe('DocToolbar', () => {
  it('labels the two views Preview and Edit, each with an icon', () => {
    const { container, dispose } = mount()
    const tabs = [...container.querySelectorAll('[role="radio"]')]
    expect(tabs.map((tab) => tab.textContent)).toEqual(['Preview', 'Edit'])
    expect(tabs.every((tab) => tab.querySelector('svg') !== null)).toBe(true)
    dispose()
  })

  it('marks the active view', () => {
    const { container, dispose } = mount()
    const checked = container.querySelector('[role="radio"][aria-checked="true"]')
    expect(checked?.textContent).toBe('Preview')
    dispose()
  })
})
