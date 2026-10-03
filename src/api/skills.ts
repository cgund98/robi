import { ApiError, errorMessage, statusOf } from './sessions'
import { api } from './client'
import type { components } from './schema'

export type SkillEntry = components['schemas']['SkillEntry']

export async function listSkills(workspaceId: string): Promise<SkillEntry[]> {
  const result = await api.GET('/api/v1/workspaces/{id}/skills', {
    params: { path: { id: workspaceId } }
  })
  if (result.data) {
    return result.data
  }
  throw new ApiError(
    statusOf(result.response as { status: number } | undefined),
    errorMessage(result.error, 'Failed to load skills')
  )
}
