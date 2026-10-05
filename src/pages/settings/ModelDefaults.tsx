import { useEffect, useState } from 'react'

import { listModels, modelDisplayName, type CatalogModel } from '../../api/models'
import { deleteSetting, getSettings, putSetting, SETTING_KEYS } from '../../api/settings'
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
    options.unshift({ value: current, label: modelDisplayName(current) })
  }
  return options
}

export function ModelDefaults() {
  const [models, setModels] = useState<CatalogModel[]>([])
  const [model, setModel] = useState('')
  const [effort, setEffort] = useState('')
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
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [catalog, settings] = await Promise.all([
          listModels(),
          getSettings([
            SETTING_KEYS.model,
            SETTING_KEYS.reasoningEffort,
            ...MODES.flatMap((item) => [item.modelKey, item.effortKey])
          ])
        ])
        const [modelSetting, effortSetting, ...modeSettings] = settings
        if (cancelled) {
          return
        }
        setModels(catalog)
        setModel(modelSetting.value ?? '')
        setEffort(effortSetting.value ?? '')
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

  async function save(key: string, value: string) {
    const trimmed = value.trim()
    if (trimmed.length === 0) {
      return
    }
    setError(null)
    try {
      await putSetting(key, trimmed, false)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  async function clear(key: string) {
    setError(null)
    try {
      await deleteSetting(key)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to clear setting')
    }
  }

  const globalModelLabel =
    models.find((item) => item.id === model)?.displayName ??
    modelDisplayName(model || 'ocg_glm-5.3')

  return (
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
              void save(SETTING_KEYS.model, value)
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
                  void save(SETTING_KEYS.reasoningEffort, value)
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
          const resolvedModelLabel = modeModelId
            ? (models.find((entry) => entry.id === modeModelId)?.displayName ??
              modelDisplayName(modeModelId))
            : 'default'
          const resolvedEffort = modeEfforts[item.mode]
            ? effortLabel(modeEfforts[item.mode])
            : 'default'
          return (
            <div key={item.mode} className={styles.defaultRow}>
              <div className={`${styles.modeName} ${styles[item.mode]}`}>{item.label}</div>
              <ChoiceMenu
                label={resolvedModelLabel}
                ariaLabel={`${item.label} model`}
                value={modeModelId}
                options={modelOptions(models, resolvedModelId)}
                defaultLabel="default"
                align="start"
                side="bottom"
                triggerClassName={`${styles.input} ${styles.select} ${styles.modelControl}`}
                onSelect={(value) => {
                  setModeModels((current) => ({ ...current, [item.mode]: value ?? '' }))
                  if (!value) {
                    void clear(item.modelKey)
                    return
                  }
                  void save(item.modelKey, value)
                }}
              />
              <ChoiceMenu
                label={resolvedEffort}
                ariaLabel={`${item.label} reasoning effort`}
                value={modeEfforts[item.mode]}
                options={EFFORT_OPTIONS}
                defaultLabel="default"
                align="end"
                side="bottom"
                triggerClassName={`${styles.input} ${styles.select} ${styles.effortControl}`}
                onSelect={(value) => {
                  setModeEfforts((current) => ({ ...current, [item.mode]: value ?? '' }))
                  if (!value) {
                    void clear(item.effortKey)
                    return
                  }
                  void save(item.effortKey, value)
                }}
              />
            </div>
          )
        })}
      </div>
      {error ? <p className={`${styles.status} ${styles.statusError}`}>{error}</p> : null}
    </section>
  )
}
