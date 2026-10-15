import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

import { deleteSetting, getSettings, putSetting, SETTING_KEYS } from '../../api/settings'
import styles from './Settings.module.css'
import { workspaces } from '../../state/workspaceStore'
import { DisplaySettings } from './DisplaySettings'
import { ModelDefaults } from './ModelDefaults'

export function GeneralSettings() {
  const active = () =>
    workspaces.workspaces.find((workspace) => workspace.id === workspaces.activeWorkspaceId) ?? null
  const [lsp, setLsp] = createSignal(true)
  const [readPaths, setReadPaths] = createSignal('')
  const [writePaths, setWritePaths] = createSignal('')
  const [pathEntries, setPathEntries] = createSignal('')
  const [openPatterns, setOpenPatterns] = createSignal('')
  const [openCommand, setOpenCommand] = createSignal('')
  const [maxIterations, setMaxIterations] = createSignal('50')
  const [subagentIterations, setSubagentIterations] = createSignal('50')
  const [subagentTimeout, setSubagentTimeout] = createSignal('120')
  const [toolTimeout, setToolTimeout] = createSignal('120')
  const [error, setError] = createSignal<string | null>(null)
  const saved = {
    read: '',
    write: '',
    entries: '',
    openPatterns: '',
    openCommand: '',
    iterations: '50',
    subagentIterations: '50',
    subagentTimeout: '120',
    toolTimeout: '120'
  }

  createEffect(() => {
    if (!workspaces.loaded) {
      void workspaces.loadWorkspaces()
    }
  })

  createEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [
          lspSetting,
          readSetting,
          writeSetting,
          entriesSetting,
          iterations,
          subagent,
          subagentTimeoutSetting,
          toolTimeoutSetting,
          openPatternsSetting,
          openCommandSetting
        ] = await getSettings([
          SETTING_KEYS.lsp,
          SETTING_KEYS.pathAllowRead,
          SETTING_KEYS.pathAllowWrite,
          SETTING_KEYS.pathEntries,
          SETTING_KEYS.maxIterations,
          SETTING_KEYS.subagentMaxIterations,
          SETTING_KEYS.subagentTimeoutSeconds,
          SETTING_KEYS.toolTimeoutSeconds,
          SETTING_KEYS.fileOpenPatterns,
          SETTING_KEYS.fileOpenCommand
        ])
        if (cancelled) {
          return
        }
        setLsp(lspSetting.value !== 'off')
        const read = readSetting.value ?? ''
        const write = writeSetting.value ?? ''
        const entries = entriesSetting.value ?? ''
        const turns = iterations.value ?? '50'
        const childTurns = subagent.value ?? '50'
        const childTimeout = subagentTimeoutSetting.value ?? '120'
        const shellTimeout = toolTimeoutSetting.value ?? '120'
        const patterns = openPatternsSetting.value ?? ''
        const command = openCommandSetting.value ?? ''
        saved.read = read
        saved.write = write
        saved.entries = entries
        saved.iterations = turns
        saved.subagentIterations = childTurns
        saved.subagentTimeout = childTimeout
        saved.toolTimeout = shellTimeout
        saved.openPatterns = patterns
        saved.openCommand = command
        setReadPaths(read)
        setWritePaths(write)
        setPathEntries(entries)
        setMaxIterations(turns)
        setSubagentIterations(childTurns)
        setSubagentTimeout(childTimeout)
        setToolTimeout(shellTimeout)
        setOpenPatterns(patterns)
        setOpenCommand(command)
      } catch (err: unknown) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load settings')
        }
      }
    })()
    onCleanup(() => {
      cancelled = true
    })
  })

  async function toggleLsp() {
    const next = !lsp()
    setLsp(next)
    setError(null)
    try {
      await putSetting(SETTING_KEYS.lsp, next ? 'on' : 'off', false)
    } catch (err: unknown) {
      setLsp(!next)
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  async function savePaths(
    key: string,
    value: string,
    savedKey: keyof typeof saved,
    restore: (value: string) => void
  ) {
    setError(null)
    const trimmed = value.trim()
    if (trimmed === saved[savedKey]) {
      restore(trimmed)
      return
    }
    try {
      if (trimmed.length === 0) {
        await deleteSetting(key)
      } else {
        await putSetting(key, trimmed, false)
      }
      saved[savedKey] = trimmed
      restore(trimmed)
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  return (
    <>
      <h1 class={styles.title}>General</h1>
      <DisplaySettings />
      <ModelDefaults />
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Workspace</h2>
        <div class={styles.card}>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Active workspace</div>
              <div class={styles.hint}>
                {active() ? active()!.root : 'No workspace selected. Open one from Workspaces.'}
              </div>
            </div>
            <input
              class={`${styles.input} ${styles.control}`}
              value={active()?.name ?? ''}
              readOnly
              aria-label="Workspace"
            />
          </div>
        </div>
      </section>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Files</h2>
        <p class={styles.sectionHint}>
          A path that matches a line opens outside the file viewer. Every other path opens in the
          file viewer. One glob per line. `*` matches any run of characters.
        </p>
        <div class={styles.card}>
          <PathBlock
            label="Open externally"
            hint="Matched against the whole workspace-relative path, such as *.rs. An empty list opens every file in the viewer."
            ariaLabel="External open patterns"
            value={openPatterns()}
            onInput={setOpenPatterns}
            onBlur={() =>
              void savePaths(
                SETTING_KEYS.fileOpenPatterns,
                openPatterns(),
                'openPatterns',
                setOpenPatterns
              )
            }
          />
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Open command</div>
              <div class={styles.hint}>
                {`Used for a path that matches. {file_path} is the file and {line} is the line. Empty uses the app the system uses for that file.`}
              </div>
            </div>
            <input
              class={`${styles.input} ${styles.control}`}
              spellcheck={false}
              aria-label="External open command"
              placeholder="zed {file_path}:{line}"
              value={openCommand()}
              onInput={(event) => setOpenCommand(event.currentTarget.value)}
              onBlur={() =>
                void savePaths(
                  SETTING_KEYS.fileOpenCommand,
                  openCommand(),
                  'openCommand',
                  setOpenCommand
                )
              }
            />
          </div>
        </div>
      </section>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Agent</h2>
        <p class={styles.sectionHint}>
          Language tools and the turn budget for the main conversation.
        </p>
        <div class={styles.card}>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Language server</div>
              <div class={styles.hint}>
                Diagnostics, definition, references, hover, and workspace symbols. The next turn
                uses this.
              </div>
            </div>
            <button
              type="button"
              class={styles.switch}
              role="switch"
              aria-checked={lsp()}
              aria-label="Language server"
              onClick={() => void toggleLsp()}
            >
              <span class={styles.knob} />
            </button>
          </div>
          <NumberRow
            label="Max iterations"
            hint="Model turns in one reply. Tool calls inside a turn do not count. The next turn uses this. From 1 to 500."
            ariaLabel="Max iterations"
            value={maxIterations()}
            onInput={setMaxIterations}
            onBlur={() => {
              if (maxIterations().trim().length === 0) {
                setMaxIterations(saved.iterations)
                return
              }
              void savePaths(
                SETTING_KEYS.maxIterations,
                maxIterations(),
                'iterations',
                setMaxIterations
              )
            }}
          />
          <NumberRow
            label="Tool timeout"
            hint="Seconds before a shell command is killed. The next command uses this. From 1 to 3600."
            ariaLabel="Tool timeout"
            value={toolTimeout()}
            onInput={setToolTimeout}
            onBlur={() => {
              if (toolTimeout().trim().length === 0) {
                setToolTimeout(saved.toolTimeout)
                return
              }
              void savePaths(
                SETTING_KEYS.toolTimeoutSeconds,
                toolTimeout(),
                'toolTimeout',
                setToolTimeout
              )
            }}
          />
        </div>
      </section>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Subagents</h2>
        <p class={styles.sectionHint}>
          Limits for one explore or general child. The next delegated task uses these.
        </p>
        <div class={styles.card}>
          <NumberRow
            label="Max iterations"
            hint="Model turns for one child. From 1 to 500."
            ariaLabel="Subagent max iterations"
            value={subagentIterations()}
            onInput={setSubagentIterations}
            onBlur={() => {
              if (subagentIterations().trim().length === 0) {
                setSubagentIterations(saved.subagentIterations)
                return
              }
              void savePaths(
                SETTING_KEYS.subagentMaxIterations,
                subagentIterations(),
                'subagentIterations',
                setSubagentIterations
              )
            }}
          />
          <NumberRow
            label="Timeout"
            hint="Seconds before the child is stopped. From 1 to 3600."
            ariaLabel="Subagent timeout"
            value={subagentTimeout()}
            onInput={setSubagentTimeout}
            onBlur={() => {
              if (subagentTimeout().trim().length === 0) {
                setSubagentTimeout(saved.subagentTimeout)
                return
              }
              void savePaths(
                SETTING_KEYS.subagentTimeoutSeconds,
                subagentTimeout(),
                'subagentTimeout',
                setSubagentTimeout
              )
            }}
          />
        </div>
      </section>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Sandbox</h2>
        <p class={styles.sectionHint}>
          Extra paths the next turn adds to the sandbox. One path per line. A path starting with ~/
          is your home directory.
        </p>
        <div class={styles.card}>
          <PathBlock
            label="Read paths"
            hint="Added to the read allow list."
            ariaLabel="Sandbox read paths"
            value={readPaths()}
            onInput={setReadPaths}
            onBlur={() =>
              void savePaths(SETTING_KEYS.pathAllowRead, readPaths(), 'read', setReadPaths)
            }
          />
          <PathBlock
            label="Write paths"
            hint="Added to the write allow list."
            ariaLabel="Sandbox write paths"
            value={writePaths()}
            onInput={setWritePaths}
            onBlur={() =>
              void savePaths(SETTING_KEYS.pathAllowWrite, writePaths(), 'write', setWritePaths)
            }
          />
          <PathBlock
            label="PATH entries"
            hint="Added to the sandbox PATH and to the read allow list."
            ariaLabel="PATH entries"
            value={pathEntries()}
            onInput={setPathEntries}
            onBlur={() =>
              void savePaths(SETTING_KEYS.pathEntries, pathEntries(), 'entries', setPathEntries)
            }
          />
        </div>
      </section>
      <Show when={error()}>
        <p class={`${styles.status} ${styles.statusError}`}>{error()}</p>
      </Show>
    </>
  )
}

function NumberRow(props: {
  label: string
  hint: string
  ariaLabel: string
  value: string
  onInput: (value: string) => void
  onBlur: () => void
}) {
  return (
    <div class={styles.row}>
      <div class={styles.copy}>
        <div class={styles.label}>{props.label}</div>
        <div class={styles.hint}>{props.hint}</div>
      </div>
      <input
        class={`${styles.input} ${styles.control} ${styles.number}`}
        inputMode="numeric"
        aria-label={props.ariaLabel}
        value={props.value}
        onInput={(event) => props.onInput(event.currentTarget.value)}
        onBlur={() => props.onBlur()}
      />
    </div>
  )
}

function PathBlock(props: {
  label: string
  hint: string
  ariaLabel: string
  value: string
  onInput: (value: string) => void
  onBlur: () => void
}) {
  return (
    <div class={styles.block}>
      <div class={styles.label}>{props.label}</div>
      <div class={styles.hint}>{props.hint}</div>
      <textarea
        class={styles.area}
        rows={4}
        spellcheck={false}
        aria-label={props.ariaLabel}
        value={props.value}
        onInput={(event) => props.onInput(event.currentTarget.value)}
        onBlur={() => props.onBlur()}
      />
    </div>
  )
}
