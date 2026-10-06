/** @jsxImportSource solid-js */
import { DropdownMenu } from '@kobalte/core/dropdown-menu'
import { ChevronDown, ChevronRight } from '../../ui/icons'
import { For } from 'solid-js'

import menuStyles from '../../../components/chat/ChoiceMenu.module.css'
import styles from '../../../components/chat/ModelEffortMenu.module.css'

const DEFAULT_VALUE = '__default__'

type Option = { value: string; label: string }

export function ModelEffortMenu(props: {
  modelLabel: string
  effortLabel: string
  modelValue: string
  effortValue: string
  models: Option[]
  efforts: Option[]
  onModelSelect: (value: string | null) => void
  onEffortSelect: (value: string | null) => void
  triggerClassName: string
}) {
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger class={props.triggerClassName} aria-label="Model and effort">
        <span class={menuStyles.triggerLabel}>
          {props.modelLabel} {props.effortLabel}
        </span>
        <span class={menuStyles.chevron} aria-hidden="true">
          <ChevronDown size={12} />
        </span>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content class={menuStyles.panel}>
          <Selector
            label="Model"
            valueLabel={props.modelLabel}
            value={props.modelValue}
            options={props.models}
            onSelect={props.onModelSelect}
          />
          <Selector
            label="Effort"
            valueLabel={props.effortLabel}
            value={props.effortValue}
            options={props.efforts}
            onSelect={props.onEffortSelect}
          />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu>
  )
}

function Selector(props: {
  label: string
  valueLabel: string
  value: string
  options: Option[]
  onSelect: (value: string | null) => void
}) {
  return (
    <DropdownMenu.Sub>
      <DropdownMenu.SubTrigger class={`${menuStyles.item} ${styles.row}`}>
        <span>{props.label}</span>
        <span class={styles.value}>
          {props.valueLabel}
          <ChevronRight size={14} />
        </span>
      </DropdownMenu.SubTrigger>
      <DropdownMenu.Portal>
        <DropdownMenu.SubContent class={menuStyles.panel}>
          <DropdownMenu.RadioGroup
            value={props.value || DEFAULT_VALUE}
            onChange={(next) => props.onSelect(next === DEFAULT_VALUE ? null : next)}
          >
            <DropdownMenu.RadioItem value={DEFAULT_VALUE} class={menuStyles.item}>
              Use default
            </DropdownMenu.RadioItem>
            <For each={props.options}>
              {(option) => (
                <DropdownMenu.RadioItem value={option.value} class={menuStyles.item}>
                  {option.label}
                </DropdownMenu.RadioItem>
              )}
            </For>
          </DropdownMenu.RadioGroup>
        </DropdownMenu.SubContent>
      </DropdownMenu.Portal>
    </DropdownMenu.Sub>
  )
}
