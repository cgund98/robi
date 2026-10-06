/** @jsxImportSource solid-js */
import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { ChevronDown } from '../../components/ui/icons'
import { For, Show } from 'solid-js'

import styles from './ChoiceMenu.module.css'

const DEFAULT_VALUE = '__default__'

export type ChoiceOption = {
  value: string
  label: string
  tone?: 'ask' | 'plan' | 'agent'
}

export function ChoiceMenu(props: {
  label: string
  ariaLabel: string
  value: string
  options: ChoiceOption[]
  onSelect: (value: string | null) => void
  triggerClassName: string
  align?: 'start' | 'end'
  includeDefault?: boolean
  defaultLabel?: string
}) {
  const includeDefault = () => props.includeDefault ?? true

  return (
    <DropdownMenu>
      <DropdownMenu.Trigger class={props.triggerClassName} aria-label={props.ariaLabel}>
        <span class={styles.triggerLabel}>{props.label}</span>
        <span class={styles.chevron} aria-hidden="true">
          <ChevronDown size={12} />
        </span>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class={styles.panel}>
          <DropdownMenu.RadioGroup
            value={props.value || DEFAULT_VALUE}
            onChange={(next) => props.onSelect(next === DEFAULT_VALUE ? null : next)}
          >
            <Show when={includeDefault()}>
              <DropdownMenu.RadioItem value={DEFAULT_VALUE} class={styles.item} closeOnSelect>
                {props.defaultLabel ?? 'Use default'}
              </DropdownMenu.RadioItem>
            </Show>
            <For each={props.options}>
              {(option) => (
                <DropdownMenu.RadioItem
                  value={option.value}
                  class={styles.item}
                  data-tone={option.tone}
                  closeOnSelect
                >
                  {option.label}
                </DropdownMenu.RadioItem>
              )}
            </For>
          </DropdownMenu.RadioGroup>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu>
  )
}
