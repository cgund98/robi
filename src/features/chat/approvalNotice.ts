import { invoke, isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { listMessages, type ChatMessage } from '../../api/messages'
import { toolSummary, type ChatToolCall } from './toolCallView'
import { chat } from '../../state/chatStore'

/** Sessions that already posted a banner for the current pause. */
const posted = new Set<string>()

export function claimPause(seen: Set<string>, sessionId: string): boolean {
  if (seen.has(sessionId)) {
    return false
  }
  seen.add(sessionId)
  return true
}

export function releasePause(seen: Set<string>, sessionId: string): void {
  seen.delete(sessionId)
}

/** The approval bar is on screen, so a banner would repeat it. */
export function windowInFront(state: {
  focused: boolean
  minimized: boolean
  visible: boolean
}): boolean {
  return state.visible && state.focused && !state.minimized
}

/** The banner body, in the same words as the approval bar. */
export function approvalNoticeBody(call: ChatToolCall | undefined): string {
  if (!call) {
    return 'A tool is waiting'
  }
  const { verb, target, range } = toolSummary(call)
  const label = [target, range].filter(Boolean).join(' ')
  return label ? `${verb} ${label}` : verb
}

export function releaseApprovalPause(sessionId: string): void {
  releasePause(posted, sessionId)
}

/**
 * One banner per pause, and only when this window is not in front.
 * A click focuses the window and selects the session.
 */
export async function postApprovalNotice(
  sessionId: string,
  callId: string | undefined
): Promise<void> {
  if (!isTauri() || !claimPause(posted, sessionId)) {
    return
  }
  const win = getCurrentWindow()
  const [focused, minimized, visible] = await Promise.all([
    win.isFocused(),
    win.isMinimized(),
    win.isVisible()
  ])
  if (windowInFront({ focused, minimized, visible })) {
    return
  }
  try {
    await invoke('show_approval_notice', {
      title: 'Robi needs approval',
      body: await noticeBody(sessionId, callId),
      session_id: sessionId
    })
  } catch {
    releasePause(posted, sessionId)
  }
}

/**
 * One banner when a turn fails and this window is not in front.
 * A click focuses the window and selects the session. The in-app notice
 * still carries the same message.
 */
export async function postTurnFailedNotice(sessionId: string, message: string): Promise<void> {
  if (!isTauri() || !message) {
    return
  }
  const win = getCurrentWindow()
  const [focused, minimized, visible] = await Promise.all([
    win.isFocused(),
    win.isMinimized(),
    win.isVisible()
  ])
  if (windowInFront({ focused, minimized, visible })) {
    return
  }
  try {
    await invoke('show_approval_notice', {
      title: 'Robi',
      body: message,
      session_id: sessionId
    })
  } catch {
    // The in-app notice already has the message.
  }
}

async function noticeBody(sessionId: string, callId: string | undefined): Promise<string> {
  const cached = findCall(chat.messagesBySession[sessionId] ?? [], callId)
  if (cached) {
    return approvalNoticeBody(cached)
  }
  try {
    const messages = await listMessages(sessionId)
    return approvalNoticeBody(findCall(messages, callId))
  } catch {
    return approvalNoticeBody(undefined)
  }
}

function findCall(messages: ChatMessage[], callId: string | undefined): ChatToolCall | undefined {
  if (!callId) {
    return undefined
  }
  for (const message of messages) {
    const call = message.tool_calls.find((item) => item.id === callId)
    if (call) {
      return call
    }
  }
  return undefined
}
