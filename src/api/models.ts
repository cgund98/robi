import { api } from './client'
import { ApiError, errorMessage, statusOf } from './sessions'

export type CatalogModel = {
  id: string
  displayName: string
}

export async function listModels(): Promise<CatalogModel[]> {
  const result = await api.GET('/api/v1/models')
  if (result.data) {
    return result.data.map((model) => ({
      id: model.id,
      displayName: model.display_name
    }))
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to list models')
  )
}
