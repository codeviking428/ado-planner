import { describe, expect, test } from 'vitest'
import { applyOverlays } from './overlays'
import type { WorkItemNode } from './types'

const node = (partial: Partial<WorkItemNode> & Pick<WorkItemNode, 'id'>): WorkItemNode => ({
  rev: 1,
  title: `WI ${partial.id}`,
  type: 'Task',
  state: 'Active',
  assignedTo: null,
  parentId: null,
  areaPath: 'Shop',
  iterationPath: 'Shop\\Sprint 1',
  startDate: null,
  targetDate: null,
  hasDateFields: true,
  fields: {},
  ...partial
})

describe('applyOverlays', () => {
  const epic = node({ id: 1, type: 'Epic', iterationPath: 'Shop\\FY26', state: 'New' })
  const feature = node({
    id: 2,
    type: 'Feature',
    parentId: 1,
    iterationPath: 'Shop\\Sprint 2',
    state: 'Active'
  })
  const task = node({
    id: 3,
    type: 'Task',
    parentId: 2,
    iterationPath: 'Shop\\Sprint 2',
    state: 'Closed',
    assignedTo: { displayName: 'Ada', uniqueName: 'ada@contoso' }
  })
  const other = node({ id: 4, type: 'Bug', iterationPath: 'Shop\\Sprint 1', state: 'Active' })
  const forest = [epic, feature, task, other]

  test('iteration overlay keeps matching nodes and in-forest ancestors', () => {
    const visible = applyOverlays(forest, {
      types: null,
      states: null,
      assignee: 'anyone',
      iterationPath: 'Shop\\Sprint 2'
    })
    expect(visible.map((n) => n.id).sort()).toEqual([1, 2, 3])
  })

  test('type overlay keeps ancestors so the tree does not orphan', () => {
    const visible = applyOverlays(forest, {
      types: ['Task'],
      states: null,
      assignee: 'anyone',
      iterationPath: null
    })
    expect(visible.map((n) => n.id).sort()).toEqual([1, 2, 3])
  })

  test('state overlay can hide Closed without dropping ancestors of remaining matches', () => {
    const visible = applyOverlays(forest, {
      types: null,
      states: ['Active', 'New'],
      assignee: 'anyone',
      iterationPath: null
    })
    expect(visible.map((n) => n.id).sort()).toEqual([1, 2, 4])
  })

  test('assignee Me keeps only that identity and ancestors', () => {
    const visible = applyOverlays(forest, {
      types: null,
      states: null,
      assignee: 'me',
      iterationPath: null,
      currentUserUniqueName: 'ada@contoso'
    })
    expect(visible.map((n) => n.id).sort()).toEqual([1, 2, 3])
  })

  test('Unassigned hides assigned Work Items', () => {
    const visible = applyOverlays(forest, {
      types: null,
      states: null,
      assignee: 'unassigned',
      iterationPath: null
    })
    expect(visible.map((n) => n.id).sort()).toEqual([1, 2, 4])
  })

  test('empty type overlay matches nothing', () => {
    const visible = applyOverlays(forest, {
      types: [],
      states: null,
      assignee: 'anyone',
      iterationPath: null
    })
    expect(visible).toEqual([])
  })
})
