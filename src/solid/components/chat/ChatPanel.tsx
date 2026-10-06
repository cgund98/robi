/** @jsxImportSource solid-js */
import { createEffect, onCleanup, Show } from 'solid-js'

import type { ChatMessage } from '../../../api/messages'
import type { CatalogModel } from '../../../api/models'
import type { AgentMode } from '../../../api/sessions'
import type { AgentPhase } from '../../../state/chatStore'
import type { AttachmentMeta, FileAttachment } from '../../../components/chat/textAttachments'
import type { PlanView } from '../../../components/chat/toolCallView'
import styles from '../../../components/chat/ChatPanel.module.css'
import { Composer } from './Composer'
import { EditReviewStrip } from './EditReviewStrip'
import { Transcript } from './Transcript'

export type ChatPanelProps = {
  messages: ChatMessage[]
  sessionId: string | null
  echo: string | null
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
  showReviewStrip?: boolean
}

export function ChatPanel(props: ChatPanelProps) {
  let thread: HTMLDivElement | undefined
  let dock: HTMLDivElement | undefined

  createEffect(() => {
    props.sessionId
    props.draftKey
    props.showReviewStrip
    const dockNode = dock
    const threadNode = thread
    if (!dockNode || !threadNode) {
      return
    }
    const apply = () => {
      threadNode.style.setProperty('--dock-height', `${dockNode.offsetHeight}px`)
      const composer = dockNode.lastElementChild
      if (composer instanceof HTMLElement) {
        threadNode.style.setProperty('--composer-height', `${composer.offsetHeight}px`)
      }
    }
    apply()
    const observer = new ResizeObserver(apply)
    observer.observe(dockNode)
    onCleanup(() => observer.disconnect())
  })

  return (
    <div class={styles.chat}>
      <div class={styles.thread} ref={thread}>
        <Transcript
          messages={props.messages}
          sessionId={props.sessionId}
          echo={props.echo}
          echoFiles={props.echoFiles}
          phase={props.phase}
          mode={props.mode}
          deciding={props.deciding}
          buildDisabled={props.buildDisabled}
          onDecide={props.onDecide}
          onBuild={props.onBuild}
          onViewPlan={props.onViewPlan}
        />
        <div class={styles.dock} ref={dock}>
          <Show when={props.showReviewStrip && props.sessionId}>
            <EditReviewStrip sessionId={props.sessionId!} />
          </Show>
          <Composer
            disabled={props.disabled}
            pending={props.pending}
            running={props.running}
            stopping={props.stopping}
            onStop={props.onStop}
            onSubmit={props.onSubmit}
            models={props.models}
            mode={props.mode}
            modelId={props.modelId}
            effort={props.effort}
            defaultModelId={props.defaultModelId}
            defaultEffort={props.defaultEffort}
            onModeChange={props.onModeChange}
            onModelChange={props.onModelChange}
            onEffortChange={props.onEffortChange}
            messages={props.messages}
            draftKey={props.draftKey}
            pendingText={props.pendingText}
            workspaceId={props.workspaceId}
            onCompact={props.onCompact}
            compacting={props.compacting}
          />
        </div>
      </div>
    </div>
  )
}
