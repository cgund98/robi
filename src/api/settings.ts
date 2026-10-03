import { api } from './client'
import { ApiError } from './sessions'

export const SETTING_KEYS = {
  apiKey: 'opencode_go_api_key',
  braveSearchApiKey: 'brave_search_api_key',
  model: 'model',
  baseUrl: 'base_url',
  reasoningEffort: 'reasoning_effort',
  modelAsk: 'model_ask',
  modelPlan: 'model_plan',
  modelAgent: 'model_agent',
  reasoningEffortAsk: 'reasoning_effort_ask',
  reasoningEffortPlan: 'reasoning_effort_plan',
  reasoningEffortAgent: 'reasoning_effort_agent'
} as const

export type SettingView = {
  key: string
  secret: boolean
  value: string | null
  /** True when a secret is stored and the value is omitted from the response. */
  configured: boolean
}

export async function getSetting(key: string): Promise<SettingView> {
  const result = await api.GET('/api/v1/settings/{key}', {
    params: { path: { key } }
  })
  if (!result.data) {
    throw new ApiError(result.response.status, 'Failed to load setting')
  }
  const value = result.data.value ?? null
  // A stored secret omits `value`. An unset key sends `null`.
  const configured = result.data.secret
    ? result.data.value !== null
    : value != null && value.length > 0
  return {
    key: result.data.key,
    secret: result.data.secret,
    value: result.data.secret ? null : value,
    configured
  }
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
