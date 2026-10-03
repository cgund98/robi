const generationByKey = new Map<string, number>()

/** Marks a fetch to `key` as the latest. An earlier fetch to that key is stale. */
export function startFetch(key: string): number {
  const next = (generationByKey.get(key) ?? 0) + 1
  generationByKey.set(key, next)
  return next
}

/** True when no later fetch to `key` has started. */
export function fetchStillCurrent(key: string, generation: number): boolean {
  return generationByKey.get(key) === generation
}
