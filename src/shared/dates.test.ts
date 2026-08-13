import { describe, expect, test } from 'vitest'
import {
  buildStartTargetPatch,
  clientRollup,
  fromGanttExclusiveEnd,
  isUnscheduled,
  isoDateOnly,
  typeHasStartAndTarget,
  toGanttInclusiveTarget
} from './dates'
import { START_DATE_FIELD, TARGET_DATE_FIELD } from './types'
import type { WorkItemNode } from './types'

describe('Unscheduled and date PATCH', () => {
  test('empty Start or Target is Unscheduled', () => {
    expect(isUnscheduled({ startDate: null, targetDate: null })).toBe(true)
    expect(isUnscheduled({ startDate: '2026-07-01T00:00:00Z', targetDate: null })).toBe(true)
    expect(
      isUnscheduled({
        startDate: '2026-07-01T00:00:00Z',
        targetDate: '2026-07-10T00:00:00Z'
      })
    ).toBe(false)
  })

  test('drag PATCH writes Start Date and Target Date only, with test /rev', () => {
    const document = buildStartTargetPatch({
      rev: 5,
      startDate: new Date('2026-07-06T00:00:00Z'),
      targetDate: new Date('2026-07-20T00:00:00Z')
    })
    expect(document).toEqual([
      { op: 'test', path: '/rev', value: 5 },
      { op: 'add', path: `/fields/${START_DATE_FIELD}`, value: '2026-07-06T00:00:00Z' },
      { op: 'add', path: `/fields/${TARGET_DATE_FIELD}`, value: '2026-07-20T00:00:00Z' }
    ])
    expect(JSON.stringify(document)).not.toContain('IterationPath')
  })

  test('ReUI exclusive end converts to inclusive Target Date', () => {
    const target = new Date('2026-07-20T00:00:00Z')
    const exclusive = toGanttInclusiveTarget(target)
    expect(isoDateOnly(exclusive)).toBe('2026-07-21T00:00:00Z')
    expect(isoDateOnly(fromGanttExclusiveEnd(exclusive))).toBe('2026-07-20T00:00:00Z')
  })

  test('parent rollup is min child Start / max child Target', () => {
    const nodes: WorkItemNode[] = [
      {
        id: 1,
        rev: 1,
        title: 'Epic',
        type: 'Epic',
        state: 'New',
        assignedTo: null,
        parentId: null,
        areaPath: 'Shop',
        iterationPath: 'Shop',
        startDate: null,
        targetDate: null,
        hasDateFields: true,
        fields: {}
      },
      {
        id: 2,
        rev: 1,
        title: 'A',
        type: 'Feature',
        state: 'Active',
        assignedTo: null,
        parentId: 1,
        areaPath: 'Shop',
        iterationPath: 'Shop',
        startDate: '2026-07-10T00:00:00Z',
        targetDate: '2026-07-12T00:00:00Z',
        hasDateFields: true,
        fields: {}
      },
      {
        id: 3,
        rev: 1,
        title: 'B',
        type: 'Feature',
        state: 'Active',
        assignedTo: null,
        parentId: 1,
        areaPath: 'Shop',
        iterationPath: 'Shop',
        startDate: '2026-07-01T00:00:00Z',
        targetDate: '2026-08-01T00:00:00Z',
        hasDateFields: true,
        fields: {}
      }
    ]
    const rollup = clientRollup(nodes, 1)
    expect(rollup?.startDate.toISOString()).toBe('2026-07-01T00:00:00.000Z')
    expect(rollup?.targetDate.toISOString()).toBe('2026-08-01T00:00:00.000Z')
  })

  test('types without Start and Target cannot be dragged', () => {
    expect(typeHasStartAndTarget(['System.Title', 'System.State'])).toBe(false)
    expect(typeHasStartAndTarget([START_DATE_FIELD, TARGET_DATE_FIELD])).toBe(true)
  })
})
