/** @jsxImportSource solid-js */
import { Tooltip } from '@kobalte/core/tooltip'
import { Check, Clipboard } from '../../components/ui/icons'
import { createSignal, Show } from 'solid-js'

import styles from './CopyMarkdownButton.module.css'

export function CopyMarkdownButton(props: { text: string }) {
  const [copied, setCopied] = createSignal(false)
  const [open, setOpen] = createSignal(false)
  const label = () => (copied() ? 'Copied' : 'Copy markdown')

  return (
    <Tooltip
      open={open()}
      onOpenChange={(next) => {
        if (!copied()) {
          setOpen(next)
        }
      }}
    >
      <Tooltip.Trigger
        class={styles.button}
        aria-label={label()}
        onClick={() => {
          void navigator.clipboard.writeText(props.text).then(() => {
            setCopied(true)
            setOpen(true)
            window.setTimeout(() => {
              setCopied(false)
              setOpen(false)
            }, 1500)
          })
        }}
      >
        <Show when={copied()} fallback={<Clipboard size={14} aria-hidden="true" />}>
          <Check size={14} aria-hidden="true" />
        </Show>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content class={styles.tooltip}>{label()}</Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip>
  )
}
