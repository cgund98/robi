export type MockFileEdit = {
  path: string
  additions: number
  deletions: number
  unread?: boolean
}

export type MockTranscriptItem =
  | { kind: 'user'; id: string; text: string }
  | { kind: 'assistant'; id: string; text: string }
  | { kind: 'activity'; id: string; text: string }
  | {
      kind: 'file-edits'
      id: string
      filesEdited: number
      additions: number
      deletions: number
      files: MockFileEdit[]
    }

/** Placeholder transcript until message HTTP exists. */
export const MOCK_TRANSCRIPT: MockTranscriptItem[] = [
  {
    kind: 'user',
    id: 'm1',
    text: "We're seeing duplicate charges when customers double-click the pay button. Can you find and fix it?"
  },
  {
    kind: 'activity',
    id: 'a1',
    text: 'Read 3 files, searched the checkout flow'
  },
  {
    kind: 'assistant',
    id: 'm2',
    text: 'The charge path posts twice when the button is not disabled after the first click. The handler in `createCharge()` should guard on an in-flight flag before calling `POST /charges`.'
  },
  {
    kind: 'activity',
    id: 'a2',
    text: 'Edited checkout files'
  },
  {
    kind: 'file-edits',
    id: 'e1',
    filesEdited: 2,
    additions: 123,
    deletions: 42,
    files: [
      { path: 'slider.tsx', additions: 83, deletions: 0, unread: true },
      { path: 'page.tsx', additions: 40, deletions: 42 },
      { path: 'background.tsx', additions: 15, deletions: 0 }
    ]
  },
  {
    kind: 'assistant',
    id: 'm3',
    text: 'I disabled the pay button for the duration of the request and added an idempotency key on the charge call. Want me to walk through the test coverage next?'
  }
]
