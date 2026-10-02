import * as DropdownMenu from '@radix-ui/react-dropdown-menu'

import styles from './ChoiceMenu.module.css'

const DEFAULT_VALUE = '__default__'

export type ChoiceOption = {
  value: string
  label: string
}

type ChoiceMenuProps = {
  label: string
  ariaLabel: string
  /** Empty string means the default item is selected. */
  value: string
  options: ChoiceOption[]
  onSelect: (value: string | null) => void
  triggerClassName: string
  align?: 'start' | 'end'
  includeDefault?: boolean
}

export function ChoiceMenu({
  label,
  ariaLabel,
  value,
  options,
  onSelect,
  triggerClassName,
  align = 'end',
  includeDefault = true
}: ChoiceMenuProps) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className={triggerClassName} aria-label={ariaLabel}>
        <span className={styles.triggerLabel}>{label}</span>
        <span className={styles.chevron} aria-hidden>
          ▾
        </span>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className={styles.panel} side="top" align={align} sideOffset={6}>
          <DropdownMenu.RadioGroup
            value={value || DEFAULT_VALUE}
            onValueChange={(next) => onSelect(next === DEFAULT_VALUE ? null : next)}
          >
            {includeDefault ? (
              <DropdownMenu.RadioItem value={DEFAULT_VALUE} className={styles.item}>
                Use default
              </DropdownMenu.RadioItem>
            ) : null}
            {options.map((option) => (
              <DropdownMenu.RadioItem
                key={option.value}
                value={option.value}
                className={styles.item}
              >
                {option.label}
              </DropdownMenu.RadioItem>
            ))}
          </DropdownMenu.RadioGroup>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}
