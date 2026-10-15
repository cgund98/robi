import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

import { getReviewFile, type ReviewFile } from '../../api/review'
import { readWorkspaceFile } from '../../api/workspaces'
import { FileViewDialog } from '../review/FileViewDialog'
import { paintSides, type PaintedToken } from '../review/highlight'
import { workspaces } from '../../state/workspaceStore'

/**
 * The in-app file viewer for a path chosen from an edit card.
 *
 * A session diff, when one exists, keeps Diff and Previous available. The
 * dialog opens on Current. A path with no diff shows that file's text.
 */
export function OpenedFile(props: {
  sessionId: string | null
  path: string | null
  onClose: () => void
}) {
  const [file, setFile] = createSignal<ReviewFile | null>(null)
  const [allowDiff, setAllowDiff] = createSignal(false)
  const [paint, setPaint] = createSignal<{ baseline: PaintedToken[][]; current: PaintedToken[][] }>(
    { baseline: [], current: [] }
  )

  createEffect(() => {
    const path = props.path
    const sessionId = props.sessionId
    setFile(null)
    if (!path) {
      return
    }
    let cancelled = false
    void loadFile(sessionId, path).then(async (loaded) => {
      if (cancelled || !loaded) {
        return
      }
      const sides = await paintSides(loaded.file.path, loaded.file.baseline, loaded.file.current)
      if (cancelled) {
        return
      }
      setAllowDiff(loaded.allowDiff)
      setPaint(sides)
      setFile(loaded.file)
    })
    onCleanup(() => {
      cancelled = true
    })
  })

  return (
    <Show when={props.path}>
      <FileViewDialog
        file={file()}
        baseline={paint().baseline}
        current={paint().current}
        initialView="current"
        allowDiff={allowDiff()}
        onClose={() => props.onClose()}
      />
    </Show>
  )
}

async function loadFile(
  sessionId: string | null,
  path: string
): Promise<{ file: ReviewFile; allowDiff: boolean } | null> {
  if (sessionId) {
    const reviewed = await getReviewFile(sessionId, path).catch(() => null)
    if (reviewed) {
      return { file: reviewed, allowDiff: true }
    }
  }
  const workspaceId = workspaces.activeWorkspaceId
  if (!workspaceId) {
    return null
  }
  try {
    const content = await readWorkspaceFile(workspaceId, path)
    return { file: currentFile(path, content), allowDiff: false }
  } catch {
    return null
  }
}

function currentFile(path: string, content: string): ReviewFile {
  return {
    path,
    status: 'modified',
    additions: 0,
    deletions: 0,
    baseline: content,
    current: content,
    lines: [],
    hunks: []
  }
}
