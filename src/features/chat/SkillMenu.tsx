/** @jsxImportSource solid-js */
import { Popover } from '@kobalte/core/popover'
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'

import { listSkills, type SkillEntry } from '../../api/skills'
import styles from './SkillMenu.module.css'

function mentionQuery(draft: string, caret: number): string | null {
  const before = draft.slice(0, caret)
  const match = /(^|\s)\/([a-z0-9-]*)$/.exec(before)
  if (!match) {
    return null
  }
  return match[2] ?? ''
}

export function SkillMenu(props: {
  workspaceId: string | null
  draft: string
  caret: number
  onInsert: (draft: string, caret: number) => void
}) {
  const query = () => mentionQuery(props.draft, props.caret)
  const asking = () => query() !== null
  const [skills, setSkills] = createSignal<SkillEntry[]>([])

  createEffect(() => {
    const workspaceId = props.workspaceId
    if (!workspaceId || !asking()) {
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
    onCleanup(() => {
      cancelled = true
    })
  })

  const rows = () => {
    const needle = (query() ?? '').toLowerCase()
    return skills().filter((skill) => {
      const haystack = `${skill.id} ${skill.label} ${skill.description}`.toLowerCase()
      return haystack.includes(needle)
    })
  }

  function choose(skill: SkillEntry) {
    const before = props.draft.slice(0, props.caret)
    const after = props.draft.slice(props.caret)
    const replaced = before.replace(/(^|\s)\/([a-z0-9-]*)$/, `$1/${skill.id} `)
    props.onInsert(replaced + after, replaced.length)
  }

  return (
    <Show when={asking() && rows().length > 0}>
      <Popover open>
        <Popover.Anchor class={styles.anchor} />
        <Popover.Portal>
          <Popover.Content class={styles.menu}>
            <For each={rows()}>
              {(skill) => (
                <button type="button" class={styles.row} onClick={() => choose(skill)}>
                  <span class={styles.label}>
                    {skill.label === skill.id ? skill.id : skill.label}
                    {skill.model_invocable ? '' : ' · manual'}
                  </span>
                  <span class={styles.meta}>
                    {skill.scope === 'project' ? 'This workspace' : 'Home'}
                  </span>
                  <span class={styles.description}>{skill.description}</span>
                </button>
              )}
            </For>
          </Popover.Content>
        </Popover.Portal>
      </Popover>
    </Show>
  )
}
