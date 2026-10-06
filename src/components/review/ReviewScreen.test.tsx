import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { ReviewFile, ReviewLine } from '../../api/review'

const { navigateSpy, sendInstructionMock } = vi.hoisted(() => ({
  navigateSpy: vi.fn(),
  sendInstructionMock: vi.fn()
}))

vi.mock('../../api/review', () => ({
  decideReview: vi.fn(),
  getReviewFile: vi.fn()
}))

vi.mock('../../app/useSessionReview', () => ({
  useSessionReview: vi.fn()
}))

vi.mock('../../state/chatStore', () => ({
  useChatStore: (selector: (state: unknown) => unknown) =>
    selector({
      sessions: [{ id: 's1', workspace_id: 'w1' }],
      reviewTickBySession: {},
      sendInstruction: sendInstructionMock
    })
}))

vi.mock('../../state/workspaceStore', () => ({
  useWorkspaceStore: (selector: (state: unknown) => unknown) =>
    selector({ workspaces: [{ id: 'w1', root: '/tmp/ws' }] })
}))

vi.mock('react-router-dom', () => ({
  useNavigate: () => navigateSpy
}))

vi.mock('../../api/sessions', () => ({
  sessionDisplayTitle: () => 'session'
}))

// Shiki pulls in a large highlighter; a stub keeps the test light.
vi.mock('./highlight', () => ({
  paintSides: async () => ({ baseline: [], current: [] })
}))

import { decideReview, getReviewFile } from '../../api/review'
import { useSessionReview } from '../../app/useSessionReview'
import { ReviewScreen } from './ReviewScreen'

const decideReviewMock = vi.mocked(decideReview)
const getReviewFileMock = vi.mocked(getReviewFile)
const useSessionReviewMock = vi.mocked(useSessionReview)

function line(
  kind: ReviewLine['kind'],
  text: string,
  old_line: number | null,
  new_line: number | null
): ReviewLine {
  return { kind, text, old_line, new_line }
}

const baseline = 'a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n'
const current = 'a\nB\nc\nd\ne\nf\ng\nH\ni\nj\nk\n'

/** Two changes four lines apart merge into one visual block with no gap. */
const mergedFile: ReviewFile = {
  path: 'a.ts',
  status: 'modified',
  additions: 2,
  deletions: 2,
  baseline,
  current,
  lines: [
    line('delete', 'b', 2, null),
    line('insert', 'B', null, 2),
    line('context', 'c', 3, 3),
    line('context', 'd', 4, 4),
    line('delete', 'h', 8, null),
    line('insert', 'H', null, 8)
  ],
  hunks: [
    { id: 'first', old_start: 1, old_count: 1, new_start: 1, new_count: 1 },
    { id: 'second', old_start: 7, old_count: 1, new_start: 7, new_count: 1 }
  ]
}

beforeEach(() => {
  vi.clearAllMocks()
  useSessionReviewMock.mockReturnValue({
    files: [{ path: 'a.ts', status: 'modified', additions: 2, deletions: 2 }],
    error: null,
    loading: false
  })
  getReviewFileMock.mockResolvedValue(mergedFile)
})

afterEach(() => {
  cleanup()
})

describe('ReviewScreen in-line decisions', () => {
  it('sends exactly one hunk id from an in-line button in a merged block', async () => {
    decideReviewMock.mockResolvedValue(undefined)

    render(<ReviewScreen sessionId="s1" />)
    // Header pair plus one pair for each of the two hunks in the block.
    await waitFor(() => expect(screen.getAllByRole('button', { name: 'Approve' })).toHaveLength(3))

    fireEvent.click(screen.getAllByRole('button', { name: 'Approve' })[1])

    await waitFor(() => expect(decideReviewMock).toHaveBeenCalledTimes(1))
    expect(decideReviewMock).toHaveBeenCalledWith('s1', 'a.ts', 'approve', 'first')
  })

  it('keeps the header button as the whole-file decision, with no hunk id', async () => {
    // The loaded body names no hunks, so only the header pair exists.
    getReviewFileMock.mockResolvedValue({ ...mergedFile, hunks: [] })
    decideReviewMock.mockResolvedValue(undefined)

    render(<ReviewScreen sessionId="s1" />)
    await waitFor(() => expect(screen.getByText('a.ts')).toBeTruthy())
    expect(screen.getAllByRole('button', { name: 'Approve' })).toHaveLength(1)

    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))
    await waitFor(() => expect(decideReviewMock).toHaveBeenCalledWith('s1', 'a.ts', 'approve'))
  })
})

describe('ReviewScreen reject with reason', () => {
  async function openReasonDialog(index: number) {
    render(<ReviewScreen sessionId="s1" />)
    await waitFor(() =>
      expect(screen.getAllByRole('button', { name: 'More reject options' })).toHaveLength(3)
    )
    fireEvent.pointerDown(screen.getAllByRole('button', { name: 'More reject options' })[index], {
      button: 0
    })
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Reject with reason' }))
  }

  it('rejects the whole file with a reason and sends the whole-file attachment', async () => {
    decideReviewMock.mockResolvedValue(undefined)
    sendInstructionMock.mockResolvedValue(true)

    await openReasonDialog(0)

    fireEvent.change(await screen.findByLabelText('Feedback'), {
      target: { value: 'keep the old name' }
    })
    fireEvent.click(screen.getByRole('button', { name: 'Reject and send' }))

    await waitFor(() =>
      expect(decideReviewMock).toHaveBeenCalledWith('s1', 'a.ts', 'reject', undefined)
    )
    expect(sendInstructionMock).toHaveBeenCalledTimes(1)
    const [text, images, files] = sendInstructionMock.mock.calls[0]
    expect(text).toContain('I rejected the change to a.ts.')
    expect(text).toContain('keep the old name')
    expect(images).toBeUndefined()
    expect(files).toHaveLength(1)
    expect(files[0].name).toBe('a.ts')
    expect(files[0].path).toBe('a.ts')
    expect(files[0].absolutePath).toBe('/tmp/ws/a.ts')
    expect(files[0].startLine).toBeUndefined()
    await waitFor(() => expect(navigateSpy).toHaveBeenCalledWith('/sessions/s1'))
  })

  it('rejects one hunk with a reason and attaches that hunk’s lines', async () => {
    decideReviewMock.mockResolvedValue(undefined)
    sendInstructionMock.mockResolvedValue(true)

    await openReasonDialog(1)

    fireEvent.change(await screen.findByLabelText('Feedback'), {
      target: { value: 'rename this' }
    })
    fireEvent.click(screen.getByRole('button', { name: 'Reject and send' }))

    await waitFor(() =>
      expect(decideReviewMock).toHaveBeenCalledWith('s1', 'a.ts', 'reject', 'first')
    )
    const files = sendInstructionMock.mock.calls[0][2]
    expect(files).toHaveLength(1)
    expect(files[0].startLine).toBe(2)
    expect(files[0].endLine).toBe(2)
  })

  it('keeps the dialog open and sends nothing for an empty reason', async () => {
    await openReasonDialog(0)

    fireEvent.click(screen.getByRole('button', { name: 'Reject and send' }))

    expect(await screen.findByText('Add a reason for the model')).toBeTruthy()
    expect(decideReviewMock).not.toHaveBeenCalled()
    expect(sendInstructionMock).not.toHaveBeenCalled()
    expect(navigateSpy).not.toHaveBeenCalled()
  })
})
