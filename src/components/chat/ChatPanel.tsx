import { useEffect, useRef } from 'react'

import type { ChatMessage } from '../../api/messages'
import type { CatalogModel } from '../../api/models'
import type { AgentMode } from '../../api/sessions'
import type { AgentPhase } from '../../state/chatStore'
import { Composer } from './Composer'
import { EditReviewStrip } from './EditReviewStrip'
import { Transcript } from './Transcript'
import type { AttachmentMeta, FileAttachment } from './textAttachments'
import type { PlanView } from './toolCallView'
import styles from './ChatPanel.module.css'

export type ChatPanelProps = {
  messages: ChatMessage[]
  sessionId: string | null
  echo: string | null
  /** Chips for the pending echo's attachments, before the stored row exists. */
  echoFiles: AttachmentMeta[]
  phase: AgentPhase
  mode: AgentMode
  deciding: boolean
  buildDisabled: boolean
  onDecide: (callId: string, decision: 'approve' | 'reject') => void
  onBuild: (path: string) => void
  onViewPlan: (plan: PlanView) => void
  disabled: boolean
  pending: boolean
  running: boolean
  stopping: boolean
  onStop: () => void
  onSubmit: (text: string, images?: File[], files?: FileAttachment[]) => Promise<boolean>
  models: CatalogModel[]
  modelId: string | null
  effort: string | null
  defaultModelId: string
  defaultEffort: string | null
  onModeChange: (mode: AgentMode) => void
  onModelChange: (model: string | null) => void
  onEffortChange: (effort: string | null) => void
  draftKey: string
  pendingText?: string | null
  workspaceId?: string | null
  onCompact?: () => void
  compacting?: boolean
  /** Show the pending edit review strip above the composer. */
  showReviewStrip?: boolean
}

export function ChatPanel({
  messages,
  sessionId,
  echo,
  echoFiles,
  phase,
  mode,
  deciding,
  buildDisabled,
  onDecide,
  onBuild,
  onViewPlan,
  disabled,
  pending,
  running,
  stopping,
  onStop,
  onSubmit,
  models,
  modelId,
  effort,
  defaultModelId,
  defaultEffort,
  onModeChange,
  onModelChange,
  onEffortChange,
  draftKey,
  pendingText = null,
  workspaceId = null,
  onCompact,
  compacting = false,
  showReviewStrip = false
}: ChatPanelProps) {
  const threadRef = useRef<HTMLDivElement>(null)
  const dockRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const dock = dockRef.current
    const thread = threadRef.current
    if (!dock || !thread) {
      return
    }
    const apply = () => {
      thread.style.setProperty('--dock-height', `${dock.offsetHeight}px`)
      const composer = dock.lastElementChild
      if (composer instanceof HTMLElement) {
        thread.style.setProperty('--composer-height', `${composer.offsetHeight}px`)
      }
    }
    apply()
    const observer = new ResizeObserver(apply)
    observer.observe(dock)
    return () => observer.disconnect()
  }, [sessionId, draftKey, showReviewStrip])

  return (
    <div className={styles.chat}>
      <div className={styles.thread} ref={threadRef}>
        <Transcript
          messages={messages}
          sessionId={sessionId}
          echo={echo}
          echoFiles={echoFiles}
          phase={phase}
          mode={mode}
          deciding={deciding}
          buildDisabled={buildDisabled}
          onDecide={onDecide}
          onBuild={onBuild}
          onViewPlan={onViewPlan}
        />
        <div className={styles.dock} ref={dockRef}>
          {showReviewStrip && sessionId ? (
            <EditReviewStrip key={sessionId} sessionId={sessionId} />
          ) : null}
          <Composer
            disabled={disabled}
            pending={pending}
            running={running}
            stopping={stopping}
            onStop={onStop}
            onSubmit={onSubmit}
            models={models}
            mode={mode}
            modelId={modelId}
            effort={effort}
            defaultModelId={defaultModelId}
            defaultEffort={defaultEffort}
            onModeChange={onModeChange}
            onModelChange={onModelChange}
            onEffortChange={onEffortChange}
            messages={messages}
            draftKey={draftKey}
            pendingText={pendingText}
            workspaceId={workspaceId}
            onCompact={onCompact}
            compacting={compacting}
          />
        </div>
      </div>
    </div>
  )
}
