import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import { ChevronDown, ChevronRight } from 'lucide-react'

import menuStyles from './ChoiceMenu.module.css'
import styles from './ModelEffortMenu.module.css'

const DEFAULT_VALUE = '__default__'

type Option = { value: string; label: string }

type ModelEffortMenuProps = {
  modelLabel: string
  effortLabel: string
  /** Empty string means the default item is selected. */
  modelValue: string
  effortValue: string
  models: Option[]
  efforts: Option[]
  onModelSelect: (value: string | null) => void
  onEffortSelect: (value: string | null) => void
  triggerClassName: string
}

export function ModelEffortMenu({
  modelLabel,
  effortLabel,
  modelValue,
  effortValue,
  models,
  efforts,
  onModelSelect,
  onEffortSelect,
  triggerClassName
}: ModelEffortMenuProps) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className={triggerClassName} aria-label="Model and effort">
        <span className={menuStyles.triggerLabel}>
          {modelLabel} {effortLabel}
        </span>
        <span className={menuStyles.chevron} aria-hidden>
          <ChevronDown size={12} strokeWidth={1.5} />
        </span>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className={menuStyles.panel} side="top" align="start" sideOffset={6}>
          <Selector
            label="Model"
            valueLabel={modelLabel}
            value={modelValue}
            options={models}
            onSelect={onModelSelect}
          />
          <Selector
            label="Effort"
            valueLabel={effortLabel}
            value={effortValue}
            options={efforts}
            onSelect={onEffortSelect}
          />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

function Selector({
  label,
  valueLabel,
  value,
  options,
  onSelect
}: {
  label: string
  valueLabel: string
  value: string
  options: Option[]
  onSelect: (value: string | null) => void
}) {
  return (
    <DropdownMenu.Sub>
      <DropdownMenu.SubTrigger className={`${menuStyles.item} ${styles.row}`}>
        <span>{label}</span>
        <span className={styles.value}>
          {valueLabel}
          <ChevronRight size={14} strokeWidth={1.5} />
        </span>
      </DropdownMenu.SubTrigger>
      <DropdownMenu.Portal>
        <DropdownMenu.SubContent className={menuStyles.panel} sideOffset={8} alignOffset={-4}>
          <DropdownMenu.RadioGroup
            value={value || DEFAULT_VALUE}
            onValueChange={(next) => onSelect(next === DEFAULT_VALUE ? null : next)}
          >
            <DropdownMenu.RadioItem value={DEFAULT_VALUE} className={menuStyles.item}>
              Use default
            </DropdownMenu.RadioItem>
            {options.map((option) => (
              <DropdownMenu.RadioItem
                key={option.value}
                value={option.value}
                className={menuStyles.item}
              >
                {option.label}
              </DropdownMenu.RadioItem>
            ))}
          </DropdownMenu.RadioGroup>
        </DropdownMenu.SubContent>
      </DropdownMenu.Portal>
    </DropdownMenu.Sub>
  )
}
