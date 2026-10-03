export type TurnUsage = {
  input: number
  output: number
  cached: number
}

/** The fields the meter reads. Tool results live on their own messages. */
export type ContextMessage = {
  content: string
  tool_calls?: { name: string; args: unknown }[]
  usage?: TurnUsage | null
}

export type ContextEstimate = {
  /** Prompt tokens from the latest report. Null until a model turn reports input. */
  reported: number | null
  /** Characters since that report, divided by four. Includes the reporting message. */
  uncounted: number
  /** Reported input plus the uncounted estimate. */
  used: number | null
  /** Share of the window, capped at 100. Null without a report or a window. */
  percent: number | null
  lastTurn: TurnUsage | null
  window: number | null
}

export function estimateContext(
  messages: ContextMessage[],
  draft: string,
  window: number | null | undefined,
  pendingText = ''
): ContextEstimate {
  const knownWindow = window != null && window > 0 ? window : null
  let start = -1
  let reported = 0
  let lastTurn: TurnUsage | null = null
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const usage = messages[index]?.usage
    if (usage && usage.input > 0) {
      reported = usage.input
      lastTurn = usage
      start = index
      break
    }
  }
  if (start < 0) {
    return {
      reported: null,
      uncounted: 0,
      used: null,
      percent: null,
      lastTurn: null,
      window: knownWindow
    }
  }

  let chars = draft.length + pendingText.length
  for (const message of messages.slice(start)) {
    chars += messageChars(message)
  }
  const uncounted = Math.floor(chars / 4)
  const used = reported + uncounted
  const percent = knownWindow == null ? null : Math.min(100, Math.floor((used * 100) / knownWindow))
  return { reported, uncounted, used, percent, lastTurn, window: knownWindow }
}

function messageChars(message: ContextMessage): number {
  let chars = message.content.length
  for (const call of message.tool_calls ?? []) {
    chars += call.name.length + JSON.stringify(call.args ?? {}).length
  }
  return chars
}

export function formatTokens(count: number): string {
  return count.toLocaleString('en-US')
}
