import type { AssigneeFilter } from './types'

export type PlannerPrefs = {
  org: string
  project: string
  team: string
  iterationPath: string
  assignee: AssigneeFilter
  hiddenTypes: string[]
  hiddenStates: string[]
  /** `undefined` = never set; `null` = Show all; `[]` = Hide all. */
  rootTypes?: string[] | null
}

function asString(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((row): row is string => typeof row === 'string') : []
}

export function parsePlannerPrefs(raw: string | null): PlannerPrefs | null {
  if (!raw) {
    return null
  }
  try {
    const value = JSON.parse(raw) as Partial<PlannerPrefs>
    const assignee =
      typeof value.assignee === 'string' && value.assignee.trim()
        ? (value.assignee.trim() as AssigneeFilter)
        : 'anyone'
    const rootTypes =
      value.rootTypes === undefined
        ? undefined
        : value.rootTypes === null
          ? null
          : Array.isArray(value.rootTypes)
            ? asStringArray(value.rootTypes)
            : undefined
    return {
      org: asString(value.org),
      project: asString(value.project),
      team: asString(value.team),
      iterationPath: asString(value.iterationPath),
      assignee,
      hiddenTypes: asStringArray(value.hiddenTypes),
      hiddenStates: asStringArray(value.hiddenStates),
      rootTypes
    }
  } catch {
    return null
  }
}

export function serializePlannerPrefs(prefs: PlannerPrefs): string {
  return JSON.stringify(prefs)
}

export function chooseAvailable(preferred: string, available: readonly string[]): string {
  if (available.length === 0) {
    return preferred
  }
  if (preferred && available.includes(preferred)) {
    return preferred
  }
  return available[0] ?? ''
}
