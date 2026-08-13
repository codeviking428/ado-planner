import { BATCH_SIZE, START_DATE_FIELD, TARGET_DATE_FIELD } from './types'
import type { WorkItemNode } from './types'

export type AreaPathValue = {
  value: string
  includeChildren: boolean
}

export type BatchWorkItem = {
  id: number
  rev?: number
  fields?: Record<string, unknown>
}

export function buildHierarchyWiql(input: {
  project: string
  types: string[]
  areas: AreaPathValue[]
}): string {
  const typeList = input.types.map((type) => `'${escapeWiql(type)}'`).join(', ')
  const areaClause = input.areas
    .map((area) => {
      const path = escapeWiql(area.value)
      return area.includeChildren
        ? `[System.AreaPath] UNDER '${path}'`
        : `[System.AreaPath] = '${path}'`
    })
    .join(' OR ')

  return [
    'SELECT [System.Id] FROM WorkItems WHERE',
    `[System.TeamProject] = '${escapeWiql(input.project)}'`,
    `AND [System.WorkItemType] IN (${typeList})`,
    `AND [System.State] <> 'Removed'`,
    areaClause ? `AND (${areaClause})` : ''
  ]
    .filter(Boolean)
    .join(' ')
}

export function escapeWiql(value: string): string {
  return value.replaceAll("'", "''")
}

export function chunkIds(ids: number[], size = BATCH_SIZE): number[][] {
  const chunks: number[][] = []
  for (let i = 0; i < ids.length; i += size) {
    chunks.push(ids.slice(i, i + size))
  }
  return chunks
}

export function identityFromField(value: unknown): WorkItemNode['assignedTo'] {
  if (!value) {
    return null
  }
  if (typeof value === 'string') {
    return { displayName: value, uniqueName: value }
  }
  if (typeof value === 'object') {
    const ref = value as { displayName?: string; uniqueName?: string; id?: string }
    if (!ref.displayName && !ref.uniqueName) {
      return null
    }
    return {
      displayName: ref.displayName ?? ref.uniqueName ?? '',
      uniqueName: ref.uniqueName ?? ref.displayName ?? '',
      id: ref.id
    }
  }
  return null
}

export function parentIdFromFields(fields: Record<string, unknown> | undefined): number | null {
  const raw = fields?.['System.Parent']
  if (typeof raw !== 'number' || raw <= 0) {
    return null
  }
  return raw
}

export function isoDateFromField(value: unknown): string | null {
  if (!value) {
    return null
  }
  if (value instanceof Date) {
    return value.toISOString()
  }
  if (typeof value === 'string') {
    return value
  }
  return null
}

export function mapBatchWorkItem(
  item: BatchWorkItem,
  typeDateFields: ReadonlySet<string>
): WorkItemNode {
  const fields = item.fields ?? {}
  const type = String(fields['System.WorkItemType'] ?? '')
  return {
    id: item.id,
    rev: item.rev ?? Number(fields['System.Rev'] ?? 1),
    title: String(fields['System.Title'] ?? ''),
    type,
    state: String(fields['System.State'] ?? ''),
    assignedTo: identityFromField(fields['System.AssignedTo']),
    parentId: parentIdFromFields(fields),
    areaPath: String(fields['System.AreaPath'] ?? ''),
    iterationPath: String(fields['System.IterationPath'] ?? ''),
    startDate: isoDateFromField(fields[START_DATE_FIELD]),
    targetDate: isoDateFromField(fields[TARGET_DATE_FIELD]),
    hasDateFields: typeDateFields.has(type),
    fields
  }
}

/** Unparented nodes and nodes whose parent is outside the loaded ID set become roots. */
export function assembleForest(nodes: WorkItemNode[]): WorkItemNode[] {
  const ids = new Set(nodes.map((node) => node.id))
  return nodes.map((node) => {
    if (node.parentId === null || !ids.has(node.parentId)) {
      return { ...node, parentId: null }
    }
    return node
  })
}

export const HIERARCHY_FIELDS = [
  'System.Id',
  'System.Rev',
  'System.Title',
  'System.WorkItemType',
  'System.State',
  'System.AssignedTo',
  'System.Parent',
  'System.AreaPath',
  'System.IterationPath',
  START_DATE_FIELD,
  TARGET_DATE_FIELD
]
