import { parsePlannerPrefs, serializePlannerPrefs, type PlannerPrefs } from '@shared/planner-prefs'

const STORAGE_KEY = 'ado-planner.prefs'

export function loadPlannerPrefs(): PlannerPrefs | null {
  try {
    return parsePlannerPrefs(localStorage.getItem(STORAGE_KEY))
  } catch {
    return null
  }
}

export function savePlannerPrefs(prefs: PlannerPrefs): void {
  localStorage.setItem(STORAGE_KEY, serializePlannerPrefs(prefs))
}
