export type UiRenderer = 'react' | 'solid'

const STORAGE_KEY = 'robi.ui'

function isRenderer(value: string | null): value is UiRenderer {
  return value === 'react' || value === 'solid'
}

/** The renderer for this load. `?ui=` wins and is stored for the next reload. */
export function readUiRenderer(): UiRenderer {
  const query = new URLSearchParams(window.location.search).get('ui')
  if (isRenderer(query)) {
    localStorage.setItem(STORAGE_KEY, query)
    return query
  }
  const stored = localStorage.getItem(STORAGE_KEY)
  return isRenderer(stored) ? stored : 'react'
}

/** A full navigation to this page with `?ui=` set, so the browser reloads that renderer. */
export function uiRendererHref(next: UiRenderer): string {
  const url = new URL(window.location.href)
  url.searchParams.set('ui', next)
  return `${url.pathname}${url.search}${url.hash}`
}
