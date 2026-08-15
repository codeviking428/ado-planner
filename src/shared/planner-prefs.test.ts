import { describe, expect, test } from 'vitest'
import { chooseAvailable, parsePlannerPrefs, serializePlannerPrefs } from './planner-prefs'

describe('planner prefs', () => {
  test('round-trips a complete selection', () => {
    const prefs = {
      org: 'amergis',
      project: 'MatterWorx',
      team: 'Platform',
      iterationPath: 'MatterWorx\\Sprint 12',
      assignee: 'me' as const,
      hiddenTypes: ['Task'],
      hiddenStates: ['Done']
    }
    expect(parsePlannerPrefs(serializePlannerPrefs(prefs))).toEqual(prefs)
  })

  test('junk or empty storage is ignored', () => {
    expect(parsePlannerPrefs(null)).toBeNull()
    expect(parsePlannerPrefs('not-json')).toBeNull()
  })

  test('blank assignee falls back to anyone', () => {
    expect(parsePlannerPrefs(JSON.stringify({ assignee: 1 }))?.assignee).toBe('anyone')
  })

  test('keeps a Team member uniqueName', () => {
    expect(parsePlannerPrefs(JSON.stringify({ assignee: 'ada@contoso.com' }))?.assignee).toBe(
      'ada@contoso.com'
    )
  })

  test('chooseAvailable keeps a still-valid preference', () => {
    expect(chooseAvailable('Platform', ['Delivery', 'Platform'])).toBe('Platform')
  })

  test('chooseAvailable keeps the preference when the list is still empty', () => {
    expect(chooseAvailable('Platform', [])).toBe('Platform')
  })

  test('chooseAvailable falls back when the preference is gone', () => {
    expect(chooseAvailable('Ghost', ['Delivery', 'Platform'])).toBe('Delivery')
  })
})
