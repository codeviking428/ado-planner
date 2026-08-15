export type BacklogLevel = {
  rank?: number
  workItemTypes?: Array<{ name?: string }>
}

function typeNames(level: BacklogLevel): string[] {
  return [
    ...new Set((level.workItemTypes ?? []).map((type) => type.name).filter(Boolean))
  ] as string[]
}

export function topBacklogTypesFromLevels(levels: readonly BacklogLevel[]): string[] {
  const withTypes = levels.filter((level) => typeNames(level).length > 0)
  if (withTypes.length === 0) {
    return []
  }
  const ranked = withTypes.filter((level) => typeof level.rank === 'number')
  const top =
    ranked.length > 0
      ? ranked.reduce((best, level) => ((level.rank ?? 0) > (best.rank ?? 0) ? level : best))
      : withTypes[0]
  return typeNames(top)
}

export function resolveRootTypes(input: {
  stored: string[] | null | undefined
  loadedTypes: readonly string[]
  topBacklogTypes: readonly string[]
}): string[] | null {
  const loaded = [...input.loadedTypes]
  if (input.stored === null) {
    return null
  }
  if (input.stored === undefined) {
    return seedRootTypes(loaded, input.topBacklogTypes)
  }
  if (input.stored.length === 0) {
    return []
  }
  const intersection = loaded.filter((type) => input.stored!.includes(type))
  if (intersection.length > 0) {
    return intersection
  }
  return seedRootTypes(loaded, input.topBacklogTypes)
}

function seedRootTypes(loaded: string[], topBacklogTypes: readonly string[]): string[] | null {
  const seed = topBacklogTypes.filter((type) => loaded.includes(type))
  return seed.length > 0 ? seed : loaded.length > 0 ? null : []
}
