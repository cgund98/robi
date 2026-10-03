import * as Popover from '@radix-ui/react-popover'
import { useEffect, useState } from 'react'

import { listSkills, type SkillEntry } from '../../api/skills'
import styles from './SkillMenu.module.css'

type SkillMenuProps = {
  workspaceId: string | null
  draft: string
  caret: number
  onInsert: (draft: string, caret: number) => void
}

/** `@` at the start of the composer or after whitespace. */
function mentionQuery(draft: string, caret: number): string | null {
  const before = draft.slice(0, caret)
  const match = /(^|\s)@([a-z0-9-]*)$/.exec(before)
  if (!match) {
    return null
  }
  return match[2] ?? ''
}

export function SkillMenu({ workspaceId, draft, caret, onInsert }: SkillMenuProps) {
  const query = mentionQuery(draft, caret)
  const [skills, setSkills] = useState<SkillEntry[]>([])
  const asking = query !== null
  const open = asking && skills.length > 0

  useEffect(() => {
    if (!workspaceId || !asking) {
      return
    }
    let cancelled = false
    void listSkills(workspaceId)
      .then((loaded) => {
        if (!cancelled) {
          setSkills(loaded)
        }
      })
      .catch(() => {
        if (!cancelled) {
          setSkills([])
        }
      })
    return () => {
      cancelled = true
    }
  }, [workspaceId, asking])

  const needle = (query ?? '').toLowerCase()
  const rows = skills.filter((skill) => {
    const haystack = `${skill.id} ${skill.label} ${skill.description}`.toLowerCase()
    return haystack.includes(needle)
  })

  function choose(skill: SkillEntry) {
    const before = draft.slice(0, caret)
    const after = draft.slice(caret)
    const replaced = before.replace(/(^|\s)@([a-z0-9-]*)$/, `$1@${skill.id} `)
    const next = replaced + after
    onInsert(next, replaced.length)
  }

  if (!open || rows.length === 0) {
    return null
  }

  return (
    <Popover.Root open>
      <Popover.Anchor className={styles.anchor} />
      <Popover.Portal>
        <Popover.Content className={styles.menu} side="top" align="start" sideOffset={8}>
          {rows.map((skill) => (
            <button key={skill.id} type="button" className={styles.row} onClick={() => choose(skill)}>
              <span className={styles.label}>
                {skill.label === skill.id ? skill.id : skill.label}
                {skill.model_invocable ? '' : ' · manual'}
              </span>
              <span className={styles.meta}>
                {skill.scope === 'project' ? 'This workspace' : 'Home'}
              </span>
              <span className={styles.description}>{skill.description}</span>
            </button>
          ))}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}
