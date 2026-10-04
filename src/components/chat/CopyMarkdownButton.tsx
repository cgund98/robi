import * as Tooltip from '@radix-ui/react-tooltip'
import { Check, Clipboard } from 'lucide-react'
import { useState } from 'react'

import styles from './CopyMarkdownButton.module.css'

type CopyMarkdownButtonProps = {
  text: string
}

export function CopyMarkdownButton({ text }: CopyMarkdownButtonProps) {
  const [copied, setCopied] = useState(false)
  const [open, setOpen] = useState(false)
  const label = copied ? 'Copied' : 'Copy markdown'

  return (
    <Tooltip.Provider delayDuration={400}>
      <Tooltip.Root
        open={open}
        onOpenChange={(next) => {
          if (!copied) {
            setOpen(next)
          }
        }}
      >
        <Tooltip.Trigger asChild>
          <button
            type="button"
            className={styles.button}
            aria-label={label}
            onClick={() => {
              void navigator.clipboard.writeText(text).then(() => {
                setCopied(true)
                setOpen(true)
                window.setTimeout(() => {
                  setCopied(false)
                  setOpen(false)
                }, 1500)
              })
            }}
          >
            {copied ? (
              <Check size={14} strokeWidth={1.6} aria-hidden />
            ) : (
              <Clipboard size={14} strokeWidth={1.3} aria-hidden />
            )}
          </button>
        </Tooltip.Trigger>
        <Tooltip.Portal>
          <Tooltip.Content className={styles.tooltip} side="top" sideOffset={6}>
            {label}
          </Tooltip.Content>
        </Tooltip.Portal>
      </Tooltip.Root>
    </Tooltip.Provider>
  )
}
