import { describe, expect, test } from 'vitest'
import {
  assembleForest,
  buildHierarchyWiql,
  chunkIds,
  mapBatchWorkItem,
  parentIdFromFields
} from './hierarchy'
import type { WorkItemNode } from './types'

const node = (partial: Partial<WorkItemNode> & Pick<WorkItemNode, 'id'>): WorkItemNode => ({
  rev: 1,
  title: `WI ${partial.id}`,
  type: 'Task',
  state: 'Active',
  assignedTo: null,
  parentId: null,
  areaPath: 'Shop\\Platform',
  iterationPath: 'Shop\\Sprint 1',
  startDate: null,
  targetDate: null,
  hasDateFields: true,
  fields: {},
  ...partial
})

describe('assembleForest', () => {
  test('unparented Work Items become roots', () => {
    const forest = assembleForest([
      node({ id: 1, parentId: null }),
      node({ id: 2, parentId: 1 }),
      node({ id: 3, parentId: 0 })
    ])
    expect(forest.find((n) => n.id === 1)?.parentId).toBeNull()
    expect(forest.find((n) => n.id === 2)?.parentId).toBe(1)
    expect(forest.find((n) => n.id === 3)?.parentId).toBeNull()
  })

  test('out-of-area parents are not kept — child becomes a root', () => {
    const forest = assembleForest([node({ id: 20, parentId: 99, title: 'in-team child' })])
    expect(forest).toEqual([expect.objectContaining({ id: 20, parentId: null })])
  })

  test('same-type nesting stays in the forest', () => {
    const forest = assembleForest([
      node({ id: 1, type: 'User Story', parentId: null }),
      node({ id: 2, type: 'User Story', parentId: 1 })
    ])
    expect(forest.find((n) => n.id === 2)?.parentId).toBe(1)
  })
})

describe('buildHierarchyWiql', () => {
  test('uses UNDER vs = from includeChildren and excludes Removed', () => {
    const wiql = buildHierarchyWiql({
      project: "O'Malley",
      types: ['Epic', 'Task'],
      areas: [
        { value: "O'Malley\\Area", includeChildren: true },
        { value: "O'Malley\\Exact", includeChildren: false }
      ]
    })
    expect(wiql).toContain("[System.TeamProject] = 'O''Malley'")
    expect(wiql).toContain("[System.AreaPath] UNDER 'O''Malley\\Area'")
    expect(wiql).toContain("[System.AreaPath] = 'O''Malley\\Exact'")
    expect(wiql).toContain("[System.State] <> 'Removed'")
    expect(wiql).toContain("IN ('Epic', 'Task')")
  })
})

describe('chunkIds', () => {
  test('batches 200 IDs', () => {
    const ids = Array.from({ length: 401 }, (_, i) => i + 1)
    const chunks = chunkIds(ids)
    expect(chunks).toHaveLength(3)
    expect(chunks[0]).toHaveLength(200)
    expect(chunks[1]).toHaveLength(200)
    expect(chunks[2]).toHaveLength(1)
  })
})

describe('mapBatchWorkItem', () => {
  test('maps System.Parent and scheduling fields', () => {
    const mapped = mapBatchWorkItem(
      {
        id: 10,
        rev: 4,
        fields: {
          'System.Title': 'Cart',
          'System.WorkItemType': 'Feature',
          'System.State': 'Active',
          'System.Parent': 1,
          'System.AssignedTo': { displayName: 'Ada', uniqueName: 'ada@contoso' },
          'Microsoft.VSTS.Scheduling.StartDate': '2026-07-01T00:00:00Z',
          'Microsoft.VSTS.Scheduling.TargetDate': '2026-07-10T00:00:00Z'
        }
      },
      new Set(['Feature'])
    )
    expect(mapped.parentId).toBe(1)
    expect(mapped.hasDateFields).toBe(true)
    expect(mapped.assignedTo).toEqual({
      displayName: 'Ada',
      uniqueName: 'ada@contoso',
      id: undefined
    })
  })

  test('missing parent is null', () => {
    expect(parentIdFromFields({})).toBeNull()
    expect(parentIdFromFields({ 'System.Parent': 0 })).toBeNull()
  })
})
