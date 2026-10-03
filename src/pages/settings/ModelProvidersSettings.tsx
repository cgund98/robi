import { Fragment, useEffect, useState } from 'react'

import { listModels, type CatalogModel } from '../../api/models'
import { deleteSetting, getSetting, putSetting, SETTING_KEYS } from '../../api/settings'
import type { AgentMode } from '../../api/sessions'
import { ChoiceMenu } from '../../components/chat/ChoiceMenu'
import styles from './Settings.module.css'

const EFFORTS = ['low', 'medium', 'high'] as const

const MODES: { mode: AgentMode; label: string; modelKey: string; effortKey: string }[] = [
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
  },
  {
    mode: 'agent',
    label: 'Agent',
    modelKey: SETTING_KEYS.modelAgent,
    effortKey: SETTING_KEYS.reasoningEffortAgent
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
  const [status, setStatus] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [modelSetting, baseSetting, effortSetting, keySetting, catalog, ...modeSettings] =
          await Promise.all([
            getSetting(SETTING_KEYS.model),
            getSetting(SETTING_KEYS.baseUrl),
            getSetting(SETTING_KEYS.reasoningEffort),
            getSetting(SETTING_KEYS.apiKey),
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
      if (secret) {
        setApiKey('')
        setKeyConfigured(true)
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

  return (
    <>
      <h1 className={styles.title}>Model Providers</h1>
      <div className={styles.card}>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>OpenCode API key</div>
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
            <div className={styles.label}>Default Model</div>
            <div className={styles.hint}>Used when a mode has no model of its own.</div>
          </div>
          <ChoiceMenu
            label={models.find((item) => item.id === model)?.displayName ?? (model || 'glm-5.3')}
            ariaLabel="Model"
            value={model}
            options={modelOptions(models, model)}
            includeDefault={false}
            align="end"
            triggerClassName={`${styles.input} ${styles.select} ${styles.control}`}
            onSelect={(value) => {
              if (!value) {
                return
              }
              setModel(value)
              void save(SETTING_KEYS.model, value, false)
            }}
          />
        </div>
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Default Reasoning effort</div>
            <div className={styles.hint}>Used when a mode has no effort of its own.</div>
          </div>
          <div
            className={`${styles.segments} ${styles.control}`}
            role="group"
            aria-label="Reasoning effort"
          >
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
        {MODES.map((item) => (
          <Fragment key={item.mode}>
            <div className={styles.row}>
              <div className={styles.copy}>
                <div className={styles.label}>{item.label} model</div>
                <div className={styles.hint}>Falls back to the default model when unset.</div>
              </div>
              <ChoiceMenu
                label={
                  models.find((entry) => entry.id === modeModels[item.mode])?.displayName ??
                  (modeModels[item.mode] || 'Default')
                }
                ariaLabel={`${item.label} model`}
                value={modeModels[item.mode]}
                options={modelOptions(models, modeModels[item.mode])}
                align="end"
                triggerClassName={`${styles.input} ${styles.select} ${styles.control}`}
                onSelect={(value) => {
                  setModeModels((current) => ({ ...current, [item.mode]: value ?? '' }))
                  if (!value) {
                    void clear(item.modelKey)
                    return
                  }
                  void save(item.modelKey, value, false)
                }}
              />
            </div>
            <div className={styles.row}>
              <div className={styles.copy}>
                <div className={styles.label}>{item.label} effort</div>
                <div className={styles.hint}>Falls back to the default effort when unset.</div>
              </div>
              <div
                className={`${styles.segments} ${styles.control}`}
                role="group"
                aria-label={`${item.label} reasoning effort`}
              >
                <button
                  type="button"
                  className={`${styles.segment} ${modeEfforts[item.mode] === '' ? styles.segmentActive : ''}`}
                  onClick={() => {
                    setModeEfforts((current) => ({ ...current, [item.mode]: '' }))
                    void clear(item.effortKey)
                  }}
                >
                  default
                </button>
                {EFFORTS.map((value) => (
                  <button
                    key={value}
                    type="button"
                    className={`${styles.segment} ${modeEfforts[item.mode] === value ? styles.segmentActive : ''}`}
                    onClick={() => {
                      setModeEfforts((current) => ({ ...current, [item.mode]: value }))
                      void save(item.effortKey, value, false)
                    }}
                  >
                    {value}
                  </button>
                ))}
              </div>
            </div>
          </Fragment>
        ))}
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
      {error ? <p className={`${styles.status} ${styles.statusError}`}>{error}</p> : null}
      {status && !error ? <p className={styles.status}>{status}</p> : null}
    </>
  )
}
