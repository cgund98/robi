import { useEffect, useState } from 'react'

import { getSettings, putSetting, SETTING_KEYS } from '../../api/settings'
import styles from './Settings.module.css'

export function ModelProvidersSettings() {
  const [baseUrl, setBaseUrl] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [keyConfigured, setKeyConfigured] = useState(false)
  const [anthropicKey, setAnthropicKey] = useState('')
  const [anthropicKeyConfigured, setAnthropicKeyConfigured] = useState(false)
  const [searchKey, setSearchKey] = useState('')
  const [searchKeyConfigured, setSearchKeyConfigured] = useState(false)
  const [openCodeEnabled, setOpenCodeEnabled] = useState(true)
  const [anthropicEnabled, setAnthropicEnabled] = useState(true)
  const [deepseekKey, setDeepseekKey] = useState('')
  const [deepseekKeyConfigured, setDeepseekKeyConfigured] = useState(false)
  const [deepseekBaseUrl, setDeepseekBaseUrl] = useState('')
  const [deepseekEnabled, setDeepseekEnabled] = useState(true)
  const [status, setStatus] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [
          baseSetting,
          keySetting,
          anthropicKeySetting,
          deepseekKeySetting,
          deepseekBaseSetting,
          searchSetting,
          openCodeProvider,
          anthropicProvider,
          deepseekProvider
        ] = await getSettings([
          SETTING_KEYS.baseUrl,
          SETTING_KEYS.apiKey,
          SETTING_KEYS.anthropicApiKey,
          SETTING_KEYS.deepseekApiKey,
          SETTING_KEYS.deepseekBaseUrl,
          SETTING_KEYS.braveSearchApiKey,
          SETTING_KEYS.providerOpenCodeGo,
          SETTING_KEYS.providerAnthropic,
          SETTING_KEYS.providerDeepseek
        ])
        if (cancelled) {
          return
        }
        setBaseUrl(baseSetting.value ?? '')
        setKeyConfigured(keySetting.configured)
        setAnthropicKeyConfigured(anthropicKeySetting.configured)
        setDeepseekKeyConfigured(deepseekKeySetting.configured)
        setDeepseekBaseUrl(deepseekBaseSetting.value ?? '')
        setSearchKeyConfigured(searchSetting.configured)
        setOpenCodeEnabled(openCodeProvider.value !== 'off')
        setAnthropicEnabled(anthropicProvider.value !== 'off')
        setDeepseekEnabled(deepseekProvider.value !== 'off')
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
      if (key === SETTING_KEYS.anthropicApiKey) {
        setAnthropicKey('')
        setAnthropicKeyConfigured(true)
      }
      if (key === SETTING_KEYS.deepseekApiKey) {
        setDeepseekKey('')
        setDeepseekKeyConfigured(true)
      }
      if (key === SETTING_KEYS.braveSearchApiKey) {
        setSearchKey('')
        setSearchKeyConfigured(true)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  async function toggleProvider(key: string, current: boolean, apply: (next: boolean) => void) {
    const next = !current
    apply(next)
    setError(null)
    setStatus(null)
    try {
      await putSetting(key, next ? 'on' : 'off', false)
      setStatus('Saved')
    } catch (err) {
      apply(!next)
      setError(err instanceof Error ? err.message : 'Failed to save setting')
    }
  }

  return (
    <>
      <h1 className={styles.title}>Model Providers</h1>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>OpenCode Go</h2>
        <p className={styles.sectionHint}>
          Models whose id begins with <code>ocg_</code>. Off hides them from the picker, and a turn
          that needs one fails.
        </p>
        <div className={styles.card}>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>Enabled</div>
            </div>
            <button
              type="button"
              className={styles.switch}
              role="switch"
              aria-checked={openCodeEnabled}
              aria-label="OpenCode Go"
              onClick={() => {
                void toggleProvider(
                  SETTING_KEYS.providerOpenCodeGo,
                  openCodeEnabled,
                  setOpenCodeEnabled
                )
              }}
            >
              <span className={styles.knob} />
            </button>
          </div>
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
              disabled={!openCodeEnabled}
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
        <h2 className={styles.sectionTitle}>Anthropic</h2>
        <p className={styles.sectionHint}>
          Models whose id begins with <code>ant_</code>. Off hides them from the picker, and a turn
          that needs one fails.
        </p>
        <div className={styles.card}>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>Enabled</div>
            </div>
            <button
              type="button"
              className={styles.switch}
              role="switch"
              aria-checked={anthropicEnabled}
              aria-label="Anthropic"
              onClick={() => {
                void toggleProvider(
                  SETTING_KEYS.providerAnthropic,
                  anthropicEnabled,
                  setAnthropicEnabled
                )
              }}
            >
              <span className={styles.knob} />
            </button>
          </div>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>API key</div>
              <div className={styles.hint}>
                {anthropicKeyConfigured
                  ? 'A key is saved. Enter a new value to replace it.'
                  : 'Stored as a secret. Turns on a Claude model fail until this is set.'}
              </div>
            </div>
            <input
              className={`${styles.input} ${styles.control}`}
              type="password"
              autoComplete="off"
              placeholder={anthropicKeyConfigured ? '••••••••' : 'API key'}
              value={anthropicKey}
              disabled={!anthropicEnabled}
              onChange={(event) => setAnthropicKey(event.target.value)}
              onBlur={() => void save(SETTING_KEYS.anthropicApiKey, anthropicKey, true)}
              aria-label="Anthropic API key"
            />
          </div>
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>DeepSeek</h2>
        <p className={styles.sectionHint}>
          Models whose id begins with <code>dsk_</code>. Off hides them from the picker, and a turn
          that needs one fails.
        </p>
        <div className={styles.card}>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>Enabled</div>
            </div>
            <button
              type="button"
              className={styles.switch}
              role="switch"
              aria-checked={deepseekEnabled}
              aria-label="DeepSeek"
              onClick={() => {
                void toggleProvider(
                  SETTING_KEYS.providerDeepseek,
                  deepseekEnabled,
                  setDeepseekEnabled
                )
              }}
            >
              <span className={styles.knob} />
            </button>
          </div>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>API key</div>
              <div className={styles.hint}>
                {deepseekKeyConfigured
                  ? 'A key is saved. Enter a new value to replace it.'
                  : 'Stored as a secret. Turns on a DeepSeek model fail until this is set.'}
              </div>
            </div>
            <input
              className={`${styles.input} ${styles.control}`}
              type="password"
              autoComplete="off"
              placeholder={deepseekKeyConfigured ? '••••••••' : 'API key'}
              value={deepseekKey}
              disabled={!deepseekEnabled}
              onChange={(event) => setDeepseekKey(event.target.value)}
              onBlur={() => void save(SETTING_KEYS.deepseekApiKey, deepseekKey, true)}
              aria-label="DeepSeek API key"
            />
          </div>
          <div className={styles.row}>
            <div className={styles.copy}>
              <div className={styles.label}>Base URL</div>
              <div className={styles.hint}>
                Optional. Leave empty to use https://api.deepseek.com.
              </div>
            </div>
            <input
              className={`${styles.input} ${styles.control}`}
              value={deepseekBaseUrl}
              placeholder="https://api.deepseek.com"
              disabled={!deepseekEnabled}
              onChange={(event) => setDeepseekBaseUrl(event.target.value)}
              onBlur={() => void save(SETTING_KEYS.deepseekBaseUrl, deepseekBaseUrl, false)}
              aria-label="DeepSeek base URL"
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
