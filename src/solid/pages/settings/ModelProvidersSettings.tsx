/** @jsxImportSource solid-js */
import { createEffect, createSignal, onCleanup, Show } from 'solid-js'

import { getSettings, putSetting, SETTING_KEYS } from '../../../api/settings'
import styles from '../../../pages/settings/Settings.module.css'

export function ModelProvidersSettings() {
  const [baseUrl, setBaseUrl] = createSignal('')
  const [apiKey, setApiKey] = createSignal('')
  const [keyConfigured, setKeyConfigured] = createSignal(false)
  const [anthropicKey, setAnthropicKey] = createSignal('')
  const [anthropicKeyConfigured, setAnthropicKeyConfigured] = createSignal(false)
  const [searchKey, setSearchKey] = createSignal('')
  const [searchKeyConfigured, setSearchKeyConfigured] = createSignal(false)
  const [openCodeEnabled, setOpenCodeEnabled] = createSignal(true)
  const [anthropicEnabled, setAnthropicEnabled] = createSignal(true)
  const [deepseekKey, setDeepseekKey] = createSignal('')
  const [deepseekKeyConfigured, setDeepseekKeyConfigured] = createSignal(false)
  const [deepseekBaseUrl, setDeepseekBaseUrl] = createSignal('')
  const [deepseekEnabled, setDeepseekEnabled] = createSignal(true)
  const [status, setStatus] = createSignal<string | null>(null)
  const [error, setError] = createSignal<string | null>(null)

  createEffect(() => {
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
    onCleanup(() => {
      cancelled = true
    })
  })

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
      <h1 class={styles.title}>Model Providers</h1>
      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>OpenCode Go</h2>
        <p class={styles.sectionHint}>
          Models whose id begins with <code>ocg_</code>. Off hides them from the picker, and a turn
          that needs one fails.
        </p>
        <div class={styles.card}>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Enabled</div>
            </div>
            <button
              type="button"
              class={styles.switch}
              role="switch"
              aria-checked={openCodeEnabled()}
              aria-label="OpenCode Go"
              onClick={() => {
                void toggleProvider(
                  SETTING_KEYS.providerOpenCodeGo,
                  openCodeEnabled(),
                  setOpenCodeEnabled
                )
              }}
            >
              <span class={styles.knob} />
            </button>
          </div>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>API key</div>
              <div class={styles.hint}>
                {keyConfigured()
                  ? 'A key is saved. Enter a new value to replace it.'
                  : 'Stored as a secret. The server never returns the value.'}
              </div>
            </div>
            <input
              class={`${styles.input} ${styles.control}`}
              type="password"
              autocomplete="off"
              placeholder={keyConfigured() ? '••••••••' : 'API key'}
              value={apiKey()}
              disabled={!openCodeEnabled()}
              onInput={(event) => setApiKey(event.currentTarget.value)}
              onBlur={() => void save(SETTING_KEYS.apiKey, apiKey(), true)}
              aria-label="OpenCode API key"
            />
          </div>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Base URL</div>
              <div class={styles.hint}>Optional. Leave empty to use the provider default.</div>
            </div>
            <input
              class={`${styles.input} ${styles.control}`}
              value={baseUrl()}
              placeholder="https://"
              onInput={(event) => setBaseUrl(event.currentTarget.value)}
              onBlur={() => void save(SETTING_KEYS.baseUrl, baseUrl(), false)}
              aria-label="Base URL"
            />
          </div>
        </div>
      </section>

      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Anthropic</h2>
        <p class={styles.sectionHint}>
          Models whose id begins with <code>ant_</code>. Off hides them from the picker, and a turn
          that needs one fails.
        </p>
        <div class={styles.card}>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Enabled</div>
            </div>
            <button
              type="button"
              class={styles.switch}
              role="switch"
              aria-checked={anthropicEnabled()}
              aria-label="Anthropic"
              onClick={() => {
                void toggleProvider(
                  SETTING_KEYS.providerAnthropic,
                  anthropicEnabled(),
                  setAnthropicEnabled
                )
              }}
            >
              <span class={styles.knob} />
            </button>
          </div>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>API key</div>
              <div class={styles.hint}>
                {anthropicKeyConfigured()
                  ? 'A key is saved. Enter a new value to replace it.'
                  : 'Stored as a secret. Turns on a Claude model fail until this is set.'}
              </div>
            </div>
            <input
              class={`${styles.input} ${styles.control}`}
              type="password"
              autocomplete="off"
              placeholder={anthropicKeyConfigured() ? '••••••••' : 'API key'}
              value={anthropicKey()}
              disabled={!anthropicEnabled()}
              onInput={(event) => setAnthropicKey(event.currentTarget.value)}
              onBlur={() => void save(SETTING_KEYS.anthropicApiKey, anthropicKey(), true)}
              aria-label="Anthropic API key"
            />
          </div>
        </div>
      </section>

      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>DeepSeek</h2>
        <p class={styles.sectionHint}>
          Models whose id begins with <code>dsk_</code>. Off hides them from the picker, and a turn
          that needs one fails.
        </p>
        <div class={styles.card}>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Enabled</div>
            </div>
            <button
              type="button"
              class={styles.switch}
              role="switch"
              aria-checked={deepseekEnabled()}
              aria-label="DeepSeek"
              onClick={() => {
                void toggleProvider(
                  SETTING_KEYS.providerDeepseek,
                  deepseekEnabled(),
                  setDeepseekEnabled
                )
              }}
            >
              <span class={styles.knob} />
            </button>
          </div>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>API key</div>
              <div class={styles.hint}>
                {deepseekKeyConfigured()
                  ? 'A key is saved. Enter a new value to replace it.'
                  : 'Stored as a secret. Turns on a DeepSeek model fail until this is set.'}
              </div>
            </div>
            <input
              class={`${styles.input} ${styles.control}`}
              type="password"
              autocomplete="off"
              placeholder={deepseekKeyConfigured() ? '••••••••' : 'API key'}
              value={deepseekKey()}
              disabled={!deepseekEnabled()}
              onInput={(event) => setDeepseekKey(event.currentTarget.value)}
              onBlur={() => void save(SETTING_KEYS.deepseekApiKey, deepseekKey(), true)}
              aria-label="DeepSeek API key"
            />
          </div>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Base URL</div>
              <div class={styles.hint}>Optional. Leave empty to use https://api.deepseek.com.</div>
            </div>
            <input
              class={`${styles.input} ${styles.control}`}
              value={deepseekBaseUrl()}
              placeholder="https://api.deepseek.com"
              disabled={!deepseekEnabled()}
              onInput={(event) => setDeepseekBaseUrl(event.currentTarget.value)}
              onBlur={() => void save(SETTING_KEYS.deepseekBaseUrl, deepseekBaseUrl(), false)}
              aria-label="DeepSeek base URL"
            />
          </div>
        </div>
      </section>

      <section class={styles.section}>
        <h2 class={styles.sectionTitle}>Web search</h2>
        <div class={styles.card}>
          <div class={styles.row}>
            <div class={styles.copy}>
              <div class={styles.label}>Brave API key</div>
              <div class={styles.hint}>
                {searchKeyConfigured()
                  ? 'A key is saved. Enter a new value to replace it. Each search waits for approval.'
                  : 'Stored as a secret. web_search fails until this is set.'}
              </div>
            </div>
            <input
              class={`${styles.input} ${styles.control}`}
              type="password"
              autocomplete="off"
              placeholder={searchKeyConfigured() ? '••••••••' : 'Brave API key'}
              value={searchKey()}
              onInput={(event) => setSearchKey(event.currentTarget.value)}
              onBlur={() => void save(SETTING_KEYS.braveSearchApiKey, searchKey(), true)}
              aria-label="Brave API key"
            />
          </div>
        </div>
      </section>

      <Show when={error()}>
        <p class={`${styles.status} ${styles.statusError}`}>{error()}</p>
      </Show>
      <Show when={status() && !error()}>
        <p class={styles.status}>{status()}</p>
      </Show>
    </>
  )
}
