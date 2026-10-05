import { api } from './client'
import { ApiError, errorMessage, statusOf } from './sessions'

export type CatalogModel = {
  id: string
  displayName: string
  contextWindow: number
}

const OPENCODE_GO_ID_PREFIX = 'ocg_'
const OPENCODE_GO_LABEL_PREFIX = 'OCG - '

/** Menu label. OpenCode Go ids share names with other catalogs, so they carry `OCG - `. */
export function modelDisplayName(id: string, displayName?: string | null): string {
  const name = displayName || id
  if (id.startsWith(OPENCODE_GO_ID_PREFIX) && !name.startsWith(OPENCODE_GO_LABEL_PREFIX)) {
    return `${OPENCODE_GO_LABEL_PREFIX}${name}`
  }
  return name
}

export async function listModels(): Promise<CatalogModel[]> {
  const result = await api.GET('/api/v1/models')
  if (result.data) {
    return result.data.map((model) => ({
      id: model.id,
      displayName: modelDisplayName(model.id, model.display_name),
      contextWindow: model.context_window
    }))
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to list models')
  )
}
