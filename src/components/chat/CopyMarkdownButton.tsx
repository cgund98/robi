import * as Tooltip from '@radix-ui/react-tooltip'
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
            {copied ? <CheckIcon /> : <ClipboardIcon />}
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

function ClipboardIcon() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden>
      <rect
        x="4"
        y="3.5"
        width="8"
        height="10"
        rx="1.2"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.3"
      />
      <path
        d="M6 3.5h4v-1a1 1 0 0 0-1-1H7a1 1 0 0 0-1 1v1z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.3"
      />
    </svg>
  )
}

function CheckIcon() {
  return (
    <svg viewBox="0 0 16 16" aria-hidden>
      <path
        d="M3.5 8.2 6.4 11l6.1-6.2"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}
