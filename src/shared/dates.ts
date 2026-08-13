import { START_DATE_FIELD, TARGET_DATE_FIELD } from './types'
import type { JsonPatchOp, WorkItemNode } from './types'

export type IterationHint = {
  startDate: string | null
  finishDate: string | null
}

export function isUnscheduled(node: Pick<WorkItemNode, 'startDate' | 'targetDate'>): boolean {
  return node.startDate === null || node.targetDate === null
}

export function toGanttInclusiveTarget(targetDate: Date): Date {
  const next = new Date(targetDate)
  next.setUTCDate(next.getUTCDate() + 1)
  return next
}

export function fromGanttExclusiveEnd(end: Date): Date {
  const next = new Date(end)
  next.setUTCDate(next.getUTCDate() - 1)
  return next
}

export function isoDateOnly(date: Date): string {
  return `${date.toISOString().slice(0, 10)}T00:00:00Z`
}

export function parseIsoDate(value: string | null): Date | null {
  if (!value) {
    return null
  }
  const parsed = new Date(value)
  return Number.isNaN(parsed.getTime()) ? null : parsed
}

export function clientRollup(
  nodes: WorkItemNode[],
  parentId: number
): { startDate: Date; targetDate: Date } | null {
  const dates: { startDate: Date; targetDate: Date }[] = []
  const childrenOf = (id: number) => nodes.filter((node) => node.parentId === id)

  const walk = (id: number) => {
    for (const child of childrenOf(id)) {
      const start = parseIsoDate(child.startDate)
      const target = parseIsoDate(child.targetDate)
      if (start && target) {
        dates.push({ startDate: start, targetDate: target })
      }
      walk(child.id)
    }
  }

  walk(parentId)
  if (dates.length === 0) {
    return null
  }

  return {
    startDate: new Date(Math.min(...dates.map((d) => d.startDate.getTime()))),
    targetDate: new Date(Math.max(...dates.map((d) => d.targetDate.getTime())))
  }
}

export function ganttBarDisplay(
  node: WorkItemNode,
  nodes: WorkItemNode[],
  iteration: IterationHint | null
): { kind: 'scheduled' | 'unscheduled' | 'no-fields'; start: Date | null; end: Date | null } {
  if (!node.hasDateFields) {
    return { kind: 'no-fields', start: null, end: null }
  }
  const start = parseIsoDate(node.startDate)
  const target = parseIsoDate(node.targetDate)
  if (start && target) {
    return { kind: 'scheduled', start, end: target }
  }
  const rollup = clientRollup(nodes, node.id)
  if (rollup) {
    return { kind: 'unscheduled', start: rollup.startDate, end: rollup.targetDate }
  }
  if (iteration?.startDate && iteration.finishDate) {
    return {
      kind: 'unscheduled',
      start: parseIsoDate(iteration.startDate),
      end: parseIsoDate(iteration.finishDate)
    }
  }
  return { kind: 'unscheduled', start: null, end: null }
}

export function buildStartTargetPatch(input: {
  rev: number
  startDate: Date
  targetDate: Date
}): JsonPatchOp[] {
  return [
    { op: 'test', path: '/rev', value: input.rev },
    { op: 'add', path: `/fields/${START_DATE_FIELD}`, value: isoDateOnly(input.startDate) },
    { op: 'add', path: `/fields/${TARGET_DATE_FIELD}`, value: isoDateOnly(input.targetDate) }
  ]
}

export function typeHasStartAndTarget(fieldReferenceNames: string[]): boolean {
  return (
    fieldReferenceNames.includes(START_DATE_FIELD) &&
    fieldReferenceNames.includes(TARGET_DATE_FIELD)
  )
}

function ymd(value: Date | string | null | undefined): string {
  if (!value) {
    return ''
  }
  const date = value instanceof Date ? value : parseIsoDate(value)
  return date ? date.toISOString().slice(0, 10) : ''
}

/** Tree-column copy. Iteration/rollup hints are display-only — never PATCHed. */
export function datesColumnText(
  node: WorkItemNode,
  nodes: WorkItemNode[],
  iteration: IterationHint | null
): string {
  if (!node.hasDateFields) {
    return 'No Start/Target on type'
  }
  if (!isUnscheduled(node)) {
    return `${ymd(node.startDate)} → ${ymd(node.targetDate)}`
  }
  const rollup = clientRollup(nodes, node.id)
  if (rollup) {
    return `Unscheduled · rollup ${ymd(rollup.startDate)}–${ymd(rollup.targetDate)}`
  }
  if (iteration?.startDate && iteration.finishDate) {
    return `Unscheduled · iteration ${ymd(iteration.startDate)}–${ymd(iteration.finishDate)}`
  }
  return 'Unscheduled'
}

export function iterationHintForPath(
  path: string,
  iterations: Array<{ path: string; startDate: string | null; finishDate: string | null }>
): IterationHint | null {
  const match = iterations.find((row) => row.path === path)
  if (!match) {
    return null
  }
  return { startDate: match.startDate, finishDate: match.finishDate }
}
