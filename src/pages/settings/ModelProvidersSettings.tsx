import { useEffect, useState } from 'react'

import { getSetting, putSetting, SETTING_KEYS } from '../../api/settings'
import styles from './Settings.module.css'

const EFFORTS = ['low', 'medium', 'high'] as const

export function ModelProvidersSettings() {
  const [model, setModel] = useState('')
  const [baseUrl, setBaseUrl] = useState('')
  const [effort, setEffort] = useState<string>('')
  const [apiKey, setApiKey] = useState('')
  const [keyConfigured, setKeyConfigured] = useState(false)
  const [status, setStatus] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void Promise.all([
      getSetting(SETTING_KEYS.model),
      getSetting(SETTING_KEYS.baseUrl),
      getSetting(SETTING_KEYS.reasoningEffort),
      getSetting(SETTING_KEYS.apiKey)
    ])
      .then(([modelSetting, baseSetting, effortSetting, keySetting]) => {
        if (cancelled) {
          return
        }
        setModel(modelSetting?.value ?? '')
        setBaseUrl(baseSetting?.value ?? '')
        setEffort(effortSetting?.value ?? '')
        setKeyConfigured(keySetting?.configured ?? false)
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load settings')
        }
      })
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
            <div className={styles.label}>Model</div>
            <div className={styles.hint}>Model id sent to the provider. Default is glm-5.3.</div>
          </div>
          <input
            className={`${styles.input} ${styles.control}`}
            value={model}
            placeholder="glm-5.3"
            onChange={(event) => setModel(event.target.value)}
            onBlur={() => void save(SETTING_KEYS.model, model, false)}
            aria-label="Model"
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
        <div className={styles.row}>
          <div className={styles.copy}>
            <div className={styles.label}>Reasoning effort</div>
            <div className={styles.hint}>
              low, medium, or high. Unset uses the provider default.
            </div>
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
      </div>
      {error ? <p className={`${styles.status} ${styles.statusError}`}>{error}</p> : null}
      {status && !error ? <p className={styles.status}>{status}</p> : null}
    </>
  )
}
