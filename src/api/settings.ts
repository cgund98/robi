import { api } from './client'
import { ApiError } from './sessions'

export const SETTING_KEYS = {
  apiKey: 'opencode_go_api_key',
  anthropicApiKey: 'anthropic_api_key',
  deepseekApiKey: 'deepseek_api_key',
  deepseekBaseUrl: 'deepseek_base_url',
  braveSearchApiKey: 'brave_search_api_key',
  model: 'model',
  baseUrl: 'base_url',
  reasoningEffort: 'reasoning_effort',
  modelAsk: 'model_ask',
  modelPlan: 'model_plan',
  modelAgent: 'model_agent',
  reasoningEffortAsk: 'reasoning_effort_ask',
  reasoningEffortPlan: 'reasoning_effort_plan',
  reasoningEffortAgent: 'reasoning_effort_agent',
  lsp: 'lsp',
  pathAllowRead: 'path_allow_read',
  pathAllowWrite: 'path_allow_write',
  pathEntries: 'path_entries',
  maxIterations: 'max_iterations',
  subagentMaxIterations: 'subagent_max_iterations',
  subagentTimeoutSeconds: 'subagent_timeout_seconds',
  toolTimeoutSeconds: 'tool_timeout_seconds',
  webSearchApproval: 'web_search_approval',
  webFetchApproval: 'web_fetch_approval',
  providerOpenCodeGo: 'provider_opencode_go',
  providerAnthropic: 'provider_anthropic',
  providerDeepseek: 'provider_deepseek'
} as const

export type SettingView = {
  key: string
  secret: boolean
  value: string | null
  /** True when a secret is stored and the value is omitted from the response. */
  configured: boolean
}

function toView(data: { key: string; secret: boolean; value?: string | null }): SettingView {
  const value = data.value ?? null
  // A stored secret omits `value`. An unset key sends `null`.
  const configured = data.secret ? data.value !== null : value != null && value.length > 0
  return {
    key: data.key,
    secret: data.secret,
    value: data.secret ? null : value,
    configured
  }
}

export async function getSetting(key: string): Promise<SettingView> {
  const result = await api.GET('/api/v1/settings/{key}', {
    params: { path: { key } }
  })
  if (!result.data) {
    throw new ApiError(result.response.status, 'Failed to load setting')
  }
  return toView(result.data)
}

export async function getSettings(keys: readonly string[]): Promise<SettingView[]> {
  const result = await api.GET('/api/v1/settings', {
    params: { query: { key: [...keys] } }
  })
  if (!result.data) {
    throw new ApiError(result.response.status, 'Failed to load settings')
  }
  return result.data.map(toView)
}

export async function putSetting(key: string, value: string, secret: boolean): Promise<void> {
  const result = await api.PUT('/api/v1/settings/{key}', {
    params: { path: { key } },
    body: { value, secret }
  })
  if (!result.response.ok) {
    throw new ApiError(result.response.status, 'Failed to save setting')
  }
}

export async function deleteSetting(key: string): Promise<void> {
  const result = await api.DELETE('/api/v1/settings/{key}', {
    params: { path: { key } }
  })
  if (!result.response.ok) {
    throw new ApiError(result.response.status, 'Failed to clear setting')
  }
}
