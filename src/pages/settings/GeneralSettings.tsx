import { useEffect, useRef, useState } from 'react'

import { deleteSetting, getSetting, putSetting, SETTING_KEYS } from '../../api/settings'
import { useWorkspaceStore } from '../../state/workspaceStore'
import styles from './Settings.module.css'

export function GeneralSettings() {
  const workspaces = useWorkspaceStore((state) => state.workspaces)
  const activeWorkspaceId = useWorkspaceStore((state) => state.activeWorkspaceId)
  const loaded = useWorkspaceStore((state) => state.loaded)
  const loadWorkspaces = useWorkspaceStore((state) => state.loadWorkspaces)
  const active = workspaces.find((workspace) => workspace.id === activeWorkspaceId) ?? null
  const [lsp, setLsp] = useState(true)
  const [readPaths, setReadPaths] = useState('')
  const [writePaths, setWritePaths] = useState('')
  const [pathEntries, setPathEntries] = useState('')
  const [maxIterations, setMaxIterations] = useState('50')
  const [subagentIterations, setSubagentIterations] = useState('50')
  const [subagentTimeout, setSubagentTimeout] = useState('120')
  const [toolTimeout, setToolTimeout] = useState('120')
  const [error, setError] = useState<string | null>(null)
  const savedRead = useRef('')
  const savedWrite = useRef('')
  const savedEntries = useRef('')
  const savedIterations = useRef('50')
  const savedSubagentIterations = useRef('50')
  const savedSubagentTimeout = useRef('120')
  const savedToolTimeout = useRef('120')

  useEffect(() => {
    if (!loaded) {
      void loadWorkspaces()
    }
  }, [loaded, loadWorkspaces])

  useEffect(() => {
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
          toolTimeoutSetting
        ] = await Promise.all([
          getSetting(SETTING_KEYS.lsp),
          getSetting(SETTING_KEYS.pathAllowRead),
          getSetting(SETTING_KEYS.pathAllowWrite),
          getSetting(SETTING_KEYS.pathEntries),
          getSetting(SETTING_KEYS.maxIterations),
          getSetting(SETTING_KEYS.subagentMaxIterations),
          getSetting(SETTING_KEYS.subagentTimeoutSeconds),
          getSetting(SETTING_KEYS.toolTimeoutSeconds)
        ])
        if (!cancelled) {
          setLsp(lspSetting.value !== 'off')
          const read = readSetting.value ?? ''
          const write = writeSetting.value ?? ''
          const entries = entriesSetting.value ?? ''
          const turns = iterations.value ?? '50'
          const childTurns = subagent.value ?? '50'
          const childTimeout = subagentTimeoutSetting.value ?? '120'
          const shellTimeout = toolTimeoutSetting.value ?? '120'
          savedRead.current = read
          savedWrite.current = write
          savedEntries.current = entries
          savedIterations.current = turns
          savedSubagentIterations.current = childTurns
          savedSubagentTimeout.current = childTimeout
          savedToolTimeout.current = shellTimeout
          setReadPaths(read)
          setWritePaths(write)
          setPathEntries(entries)
          setMaxIterations(turns)
          setSubagentIterations(childTurns)
          setSubagentTimeout(childTimeout)
          setToolTimeout(shellTimeout)
        }
      } catch (err: unknown) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load settings')
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  async function toggleLsp() {
    const next = !lsp
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
    saved: { current: string },
    restore: (value: string) => void
  ) {
    setError(null)
    const trimmed = value.trim()
    if (trimmed === saved.current) {
      restore(trimmed)
      return
    }
    try {
      if (trimmed.length === 0) {
        await deleteSetting(key)
      } else {
        await putSetting(key, trimmed, false)
      }
      saved.current = trimmed
      restore(trimmed)
    } catch (err: unknown) {
      restore(saved.current)
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  return (
    <>
      <h1 className={styles.title}>General</h1>
      <div className={styles.card}>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Workspace</div>
            <div className={styles.hint}>
              {active ? active.root : 'No workspace selected. Open one from Workspaces.'}
            </div>
          </div>
          <input
            className={`${styles.input} ${styles.control}`}
            value={active?.name ?? ''}
            readOnly
            aria-label="Workspace"
          />
        </div>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Language server</div>
            <div className={styles.hint}>
              Diagnostics, definition, references, hover, and workspace symbols. The next turn uses
              this.
            </div>
          </div>
          <button
            type="button"
            className={styles.switch}
            role="switch"
            aria-checked={lsp}
            aria-label="Language server"
            onClick={() => {
              void toggleLsp()
            }}
          >
            <span className={styles.knob} />
          </button>
        </div>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Max iterations</div>
            <div className={styles.hint}>
              Model turns in one reply. Tool calls inside a turn do not count. The next turn uses
              this. From 1 to 500.
            </div>
          </div>
          <input
            className={`${styles.input} ${styles.control} ${styles.number}`}
            inputMode="numeric"
            aria-label="Max iterations"
            value={maxIterations}
            onChange={(event) => setMaxIterations(event.target.value)}
            onBlur={() => {
              if (maxIterations.trim().length === 0) {
                setMaxIterations(savedIterations.current)
                return
              }
              void savePaths(
                SETTING_KEYS.maxIterations,
                maxIterations,
                savedIterations,
                setMaxIterations
              )
            }}
          />
        </div>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Subagent max iterations</div>
            <div className={styles.hint}>
              Model turns for one explore or general child. The next delegated task uses this. From
              1 to 500.
            </div>
          </div>
          <input
            className={`${styles.input} ${styles.control} ${styles.number}`}
            inputMode="numeric"
            aria-label="Subagent max iterations"
            value={subagentIterations}
            onChange={(event) => setSubagentIterations(event.target.value)}
            onBlur={() => {
              if (subagentIterations.trim().length === 0) {
                setSubagentIterations(savedSubagentIterations.current)
                return
              }
              void savePaths(
                SETTING_KEYS.subagentMaxIterations,
                subagentIterations,
                savedSubagentIterations,
                setSubagentIterations
              )
            }}
          />
        </div>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Subagent timeout</div>
            <div className={styles.hint}>
              Seconds before an explore or general child is stopped. The next delegated task uses
              this. From 1 to 3600.
            </div>
          </div>
          <input
            className={`${styles.input} ${styles.control} ${styles.number}`}
            inputMode="numeric"
            aria-label="Subagent timeout"
            value={subagentTimeout}
            onChange={(event) => setSubagentTimeout(event.target.value)}
            onBlur={() => {
              if (subagentTimeout.trim().length === 0) {
                setSubagentTimeout(savedSubagentTimeout.current)
                return
              }
              void savePaths(
                SETTING_KEYS.subagentTimeoutSeconds,
                subagentTimeout,
                savedSubagentTimeout,
                setSubagentTimeout
              )
            }}
          />
        </div>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Tool timeout</div>
            <div className={styles.hint}>
              Seconds before a shell command is killed. The next command uses this. From 1 to 3600.
            </div>
          </div>
          <input
            className={`${styles.input} ${styles.control} ${styles.number}`}
            inputMode="numeric"
            aria-label="Tool timeout"
            value={toolTimeout}
            onChange={(event) => setToolTimeout(event.target.value)}
            onBlur={() => {
              if (toolTimeout.trim().length === 0) {
                setToolTimeout(savedToolTimeout.current)
                return
              }
              void savePaths(
                SETTING_KEYS.toolTimeoutSeconds,
                toolTimeout,
                savedToolTimeout,
                setToolTimeout
              )
            }}
          />
        </div>
        <div className={styles.block}>
          <div className={styles.label}>Sandbox read paths</div>
          <div className={styles.hint}>
            One path per line. A path starting with ~/ is your home directory. The next turn adds
            these to the read allow list.
          </div>
          <textarea
            className={styles.area}
            rows={4}
            spellCheck={false}
            aria-label="Sandbox read paths"
            value={readPaths}
            onChange={(event) => setReadPaths(event.target.value)}
            onBlur={() => {
              void savePaths(SETTING_KEYS.pathAllowRead, readPaths, savedRead, setReadPaths)
            }}
          />
        </div>
        <div className={styles.block}>
          <div className={styles.label}>Sandbox write paths</div>
          <div className={styles.hint}>
            One path per line. A path starting with ~/ is your home directory. The next turn adds
            these to the write allow list.
          </div>
          <textarea
            className={styles.area}
            rows={4}
            spellCheck={false}
            aria-label="Sandbox write paths"
            value={writePaths}
            onChange={(event) => setWritePaths(event.target.value)}
            onBlur={() => {
              void savePaths(SETTING_KEYS.pathAllowWrite, writePaths, savedWrite, setWritePaths)
            }}
          />
        </div>
        <div className={styles.block}>
          <div className={styles.label}>PATH entries</div>
          <div className={styles.hint}>
            One directory per line. A path starting with ~/ is your home directory. These are added
            to the sandbox PATH and to the read allow list.
          </div>
          <textarea
            className={styles.area}
            rows={4}
            spellCheck={false}
            aria-label="PATH entries"
            value={pathEntries}
            onChange={(event) => setPathEntries(event.target.value)}
            onBlur={() => {
              void savePaths(SETTING_KEYS.pathEntries, pathEntries, savedEntries, setPathEntries)
            }}
          />
        </div>
      </div>
      {error ? <p className={`${styles.status} ${styles.statusError}`}>{error}</p> : null}
    </>
  )
}
