import { useEffect, useState } from 'react'

import { listModels, type CatalogModel } from '../../api/models'
import { deleteSetting, getSetting, putSetting, SETTING_KEYS } from '../../api/settings'
import type { AgentMode } from '../../api/sessions'
import { ChoiceMenu } from '../../components/chat/ChoiceMenu'
import styles from './Settings.module.css'

const EFFORTS = ['low', 'medium', 'high'] as const

const EFFORT_OPTIONS = EFFORTS.map((value) => ({ value, label: value }))

function effortLabel(value: string): string {
  return EFFORTS.find((effort) => effort === value) ?? 'medium'
}

const MODES: { mode: AgentMode; label: string; modelKey: string; effortKey: string }[] = [
  {
    mode: 'agent',
    label: 'Agent',
    modelKey: SETTING_KEYS.modelAgent,
    effortKey: SETTING_KEYS.reasoningEffortAgent
  },
  {
    mode: 'ask',
    label: 'Ask',
    modelKey: SETTING_KEYS.modelAsk,
    effortKey: SETTING_KEYS.reasoningEffortAsk
  },
  {
    mode: 'plan',
    label: 'Plan',
    modelKey: SETTING_KEYS.modelPlan,
    effortKey: SETTING_KEYS.reasoningEffortPlan
  }
]

function modelOptions(models: CatalogModel[], current: string): { value: string; label: string }[] {
  const options = models.map((item) => ({ value: item.id, label: item.displayName }))
  if (current && !options.some((option) => option.value === current)) {
    options.unshift({ value: current, label: current })
  }
  return options
}

export function ModelProvidersSettings() {
  const [models, setModels] = useState<CatalogModel[]>([])
  const [model, setModel] = useState('')
  const [baseUrl, setBaseUrl] = useState('')
  const [effort, setEffort] = useState<string>('')
  const [modeModels, setModeModels] = useState<Record<AgentMode, string>>({
    ask: '',
    plan: '',
    agent: ''
  })
  const [modeEfforts, setModeEfforts] = useState<Record<AgentMode, string>>({
    ask: '',
    plan: '',
    agent: ''
  })
  const [apiKey, setApiKey] = useState('')
  const [keyConfigured, setKeyConfigured] = useState(false)
  const [searchKey, setSearchKey] = useState('')
  const [searchKeyConfigured, setSearchKeyConfigured] = useState(false)
  const [status, setStatus] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [
          modelSetting,
          baseSetting,
          effortSetting,
          keySetting,
          searchSetting,
          catalog,
          ...modeSettings
        ] = await Promise.all([
          getSetting(SETTING_KEYS.model),
          getSetting(SETTING_KEYS.baseUrl),
          getSetting(SETTING_KEYS.reasoningEffort),
          getSetting(SETTING_KEYS.apiKey),
          getSetting(SETTING_KEYS.braveSearchApiKey),
          listModels(),
          ...MODES.flatMap((item) => [getSetting(item.modelKey), getSetting(item.effortKey)])
        ])
        if (cancelled) {
          return
        }
        setModels(catalog)
        setModel(modelSetting.value ?? '')
        setBaseUrl(baseSetting.value ?? '')
        setEffort(effortSetting.value ?? '')
        setKeyConfigured(keySetting.configured)
        setSearchKeyConfigured(searchSetting.configured)
        const nextModels = { ask: '', plan: '', agent: '' }
        const nextEfforts = { ask: '', plan: '', agent: '' }
        MODES.forEach((item, index) => {
          nextModels[item.mode] = modeSettings[index * 2]?.value ?? ''
          nextEfforts[item.mode] = modeSettings[index * 2 + 1]?.value ?? ''
        })
        setModeModels(nextModels)
        setModeEfforts(nextEfforts)
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

  async function save(key: string, value: string, secret: boolean) {
    const trimmed = value.trim()
    if (trimmed.length === 0) {
      return
    }
    setError(null)
    setStatus(null)
    try {
      await putSetting(key, trimmed, secret)
      setStatus('Saved')
      if (key === SETTING_KEYS.apiKey) {
        setApiKey('')
        setKeyConfigured(true)
      }
      if (key === SETTING_KEYS.braveSearchApiKey) {
        setSearchKey('')
        setSearchKeyConfigured(true)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  async function clear(key: string) {
    setError(null)
    setStatus(null)
    try {
      await deleteSetting(key)
      setStatus('Saved')
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to clear setting')
    }
  }

  const globalModelLabel =
    models.find((item) => item.id === model)?.displayName ?? (model || 'glm-5.3')

  return (
    <>
      <h1 className={styles.title}>Model Providers</h1>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>Model Defaults</h2>
        <p className={styles.sectionHint}>Unset modes use the Global model and effort.</p>
        <div className={styles.card}>
          <div className={styles.defaultRow}>
            <div className={styles.modeName}>Global</div>
            <ChoiceMenu
              label={globalModelLabel}
              ariaLabel="Global model"
              value={model}
              options={modelOptions(models, model)}
              includeDefault={false}
              align="start"
              triggerClassName={`${styles.input} ${styles.select} ${styles.modelControl}`}
              onSelect={(value) => {
                if (!value) {
                  return
                }
                setModel(value)
                void save(SETTING_KEYS.model, value, false)
              }}
            />
            <div className={styles.segments} role="group" aria-label="Global reasoning effort">
              {EFFORTS.map((value) => (
                <button
                  key={value}
                  type="button"
                  className={`${styles.segment} ${effort === value ? styles.segmentActive : ''}`}
                  onClick={() => {
                    setEffort(value)
                    void save(SETTING_KEYS.reasoningEffort, value, false)
                  }}
                >
                  {value}
                </button>
              ))}
            </div>
          </div>
          {MODES.map((item) => {
            const modeModelId = modeModels[item.mode]
            const resolvedModelId = modeModelId || model
            const resolvedModelLabel =
              models.find((entry) => entry.id === resolvedModelId)?.displayName ??
              (resolvedModelId || globalModelLabel)
            const resolvedEffort = effortLabel(modeEfforts[item.mode] || effort)
            return (
              <div key={item.mode} className={styles.defaultRow}>
                <div className={`${styles.modeName} ${styles[item.mode]}`}>{item.label}</div>
                <ChoiceMenu
                  label={resolvedModelLabel}
                  ariaLabel={`${item.label} model`}
                  value={modeModelId}
                  options={modelOptions(models, resolvedModelId)}
                  defaultLabel={`Default · ${globalModelLabel}`}
                  align="start"
                  side="bottom"
                  triggerClassName={`${styles.input} ${styles.select} ${styles.modelControl}`}
                  onSelect={(value) => {
                    setModeModels((current) => ({ ...current, [item.mode]: value ?? '' }))
                    if (!value) {
                      void clear(item.modelKey)
                      return
                    }
                    void save(item.modelKey, value, false)
                  }}
                />
                <ChoiceMenu
                  label={resolvedEffort}
                  ariaLabel={`${item.label} reasoning effort`}
                  value={modeEfforts[item.mode]}
                  options={EFFORT_OPTIONS}
                  defaultLabel={`Default · ${effortLabel(effort)}`}
                  align="end"
                  side="bottom"
                  triggerClassName={`${styles.input} ${styles.select} ${styles.effortControl}`}
                  onSelect={(value) => {
                    setModeEfforts((current) => ({ ...current, [item.mode]: value ?? '' }))
                    if (!value) {
                      void clear(item.effortKey)
                      return
                    }
                    void save(item.effortKey, value, false)
                  }}
                />
              </div>
            )
          })}
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>OpenCode</h2>
        <div className={styles.card}>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>API key</div>
              <div className={styles.hint}>
                {keyConfigured
                  ? 'A key is saved. Enter a new value to replace it.'
                  : 'Stored as a secret. The server never returns the value.'}
              </div>
            </div>
            <input
              className={`${styles.input} ${styles.control}`}
              type="password"
              autoComplete="off"
              placeholder={keyConfigured ? '••••••••' : 'API key'}
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
              onBlur={() => void save(SETTING_KEYS.apiKey, apiKey, true)}
              aria-label="OpenCode API key"
            />
          </div>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>Base URL</div>
              <div className={styles.hint}>Optional. Leave empty to use the provider default.</div>
            </div>
            <input
              className={`${styles.input} ${styles.control}`}
              value={baseUrl}
              placeholder="https://"
              onChange={(event) => setBaseUrl(event.target.value)}
              onBlur={() => void save(SETTING_KEYS.baseUrl, baseUrl, false)}
              aria-label="Base URL"
            />
          </div>
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>Web search</h2>
        <div className={styles.card}>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>Brave API key</div>
              <div className={styles.hint}>
                {searchKeyConfigured
                  ? 'A key is saved. Enter a new value to replace it. Each search waits for approval.'
                  : 'Stored as a secret. web_search fails until this is set.'}
              </div>
            </div>
            <input
              className={`${styles.input} ${styles.control}`}
              type="password"
              autoComplete="off"
              placeholder={searchKeyConfigured ? '••••••••' : 'Brave API key'}
              value={searchKey}
              onChange={(event) => setSearchKey(event.target.value)}
              onBlur={() => void save(SETTING_KEYS.braveSearchApiKey, searchKey, true)}
              aria-label="Brave API key"
            />
          </div>
        </div>
      </section>

      {error ? <p className={`${styles.status} ${styles.statusError}`}>{error}</p> : null}
      {status && !error ? <p className={styles.status}>{status}</p> : null}
    </>
  )
}
