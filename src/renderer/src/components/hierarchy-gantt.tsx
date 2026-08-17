import { useMemo, useRef, type ReactNode } from 'react'
import { toast } from 'sonner'
import { Gantt, type GanttColumn } from '@/components/reui/gantt/gantt'
import { GanttNav } from '@/components/reui/gantt/gantt-nav'
import type {
  GanttEvent,
  GanttProposedUpdate,
  GanttResource
} from '@/components/reui/gantt/gantt-types'
import { GanttView } from '@/components/reui/gantt/gantt-view'
import { useDependencyPrototype } from '@/components/dependency-prototype'
import { showErrorToast } from '@/lib/error-toast'
import { colorForType } from '@shared/flavor'
import {
  datesColumnText,
  fromGanttExclusiveEnd,
  isoDateOnly,
  iterationHintForPath,
  toGanttInclusiveTarget,
  type IterationHint
} from '@shared/dates'
import type { ScopeSelection, WorkItemNode } from '@shared/types'

function TypeBadge({ type }: { type: string }) {
  const color = colorForType(type)
  return (
    <span
      className="inline-flex max-w-full min-w-0 items-center gap-1.5 rounded-md px-1.5 py-0.5"
      style={{ backgroundColor: `color-mix(in oklch, ${color} 18%, transparent)` }}
      title={type}
    >
      <span className="size-1.5 shrink-0 rounded-full" style={{ backgroundColor: color }} />
      <span className="truncate text-[12px] font-medium" style={{ color }}>
        {type}
      </span>
    </span>
  )
}

function DatesCell({ text }: { text: string }): ReactNode {
  if (text.startsWith('Unscheduled')) {
    const hint = text.replace(/^Unscheduled(?: · )?(?:iteration |rollup )?/, '')
    return (
      <span className="flex min-w-0 flex-col gap-0.5 leading-tight" title={text}>
        <span className="text-muted-foreground">Unscheduled</span>
        {hint ? <span className="truncate tabular-nums">{hint}</span> : null}
      </span>
    )
  }
  return (
    <span className="truncate tabular-nums" title={text}>
      {text}
    </span>
  )
}

function toResources(items: WorkItemNode[]): GanttResource[] {
  const byParent = new Map<number | null, WorkItemNode[]>()
  for (const item of items) {
    const list = byParent.get(item.parentId) ?? []
    list.push(item)
    byParent.set(item.parentId, list)
  }
  const node = (item: WorkItemNode): GanttResource => ({
    id: String(item.id),
    title: item.title,
    color: colorForType(item.type),
    children: (byParent.get(item.id) ?? []).map(node)
  })
  return (byParent.get(null) ?? []).map(node)
}

function toEvents(items: WorkItemNode[]): GanttEvent[] {
  return items
    .filter((item) => item.hasDateFields && item.startDate && item.targetDate)
    .map((item) => ({
      id: String(item.id),
      title: item.title,
      start: new Date(item.startDate as string),
      end: toGanttInclusiveTarget(new Date(item.targetDate as string)),
      allDay: true,
      resourceId: String(item.id),
      color: colorForType(item.type)
    }))
}

export function HierarchyGantt({
  scope,
  items,
  iterations = [],
  loading = false,
  onItemsChange,
  onOpen
}: {
  scope: ScopeSelection
  items: WorkItemNode[]
  iterations?: Array<{ path: string; startDate: string | null; finishDate: string | null }>
  loading?: boolean
  onItemsChange: (items: WorkItemNode[]) => void
  onOpen: (id: number) => void
}) {
  const itemsRef = useRef(items)
  itemsRef.current = items
  const byId = useMemo(() => new Map(items.map((item) => [item.id, item])), [items])
  const resources = useMemo(() => toResources(items), [items])
  const events = useMemo(() => toEvents(items), [items])

  const proto = useDependencyPrototype(items)

  const columns: GanttColumn[] = useMemo(
    () => [
      {
        id: 'type',
        title: 'Type',
        width: 156,
        className: 'min-w-0',
        render: ({ resource }) => {
          const type = byId.get(Number(resource.id))?.type
          return type ? <TypeBadge type={type} /> : null
        }
      },
      {
        id: 'dates',
        title: 'Start / Target',
        width: 176,
        className: 'min-w-0',
        render: ({ resource }) => {
          const item = byId.get(Number(resource.id))
          if (!item) {
            return null
          }
          const hint: IterationHint | null = iterationHintForPath(item.iterationPath, iterations)
          return (
            <span data-testid={`dates-${item.id}`} className="min-w-0">
              <DatesCell text={datesColumnText(item, items, hint)} />
            </span>
          )
        }
      }
    ],
    [byId, items, iterations]
  )

  const handleEventUpdate = (update: GanttProposedUpdate) => {
    const id = Number(update.event.resourceId ?? update.event.id)
    const item = itemsRef.current.find((row) => row.id === id)
    if (!item) {
      return false
    }
    if (!item.hasDateFields) {
      showErrorToast(`#${item.id} ${item.type} has no Start Date / Target Date`)
      return false
    }
    const previousStart = item.startDate
    const previousTarget = item.targetDate
    const previousRev = item.rev
    const nextStart = update.start
    const nextTarget = fromGanttExclusiveEnd(update.end)
    onItemsChange(
      itemsRef.current.map((row) =>
        row.id === id
          ? { ...row, startDate: nextStart.toISOString(), targetDate: nextTarget.toISOString() }
          : row
      )
    )
    void window.planner.ado
      .patchDates({
        org: scope.org,
        project: scope.project,
        id,
        rev: previousRev,
        startDate: isoDateOnly(nextStart),
        targetDate: isoDateOnly(nextTarget)
      })
      .then((result) => {
        onItemsChange(
          itemsRef.current.map((row) => (row.id === id ? { ...row, rev: result.rev } : row))
        )
        toast.success(`Saved Start Date and Target Date on #${item.id} ${item.title}`)
      })
      .catch((error: unknown) => {
        onItemsChange(
          itemsRef.current.map((row) =>
            row.id === id ? { ...row, startDate: previousStart, targetDate: previousTarget } : row
          )
        )
        showErrorToast(error, 'PATCH failed')
      })
    return true
  }

  if (items.length === 0 && !loading) {
    return (
      <div
        className="text-muted-foreground flex h-full items-center justify-center text-sm"
        data-testid="empty-gantt"
      >
        No Work Items match this Team and the current filters.
      </div>
    )
  }

  return (
    <div className="relative h-full">
      <Gantt
        events={events}
        resources={resources}
        loading={loading}
        defaultScale="month"
        className="h-full text-[13px]"
        summaryBars
        parentScheduling={false}
        rowCheckboxes={false}
        dragCreate={false}
        displayScheduleHint={false}
        metrics={{ minRowHeight: 2.75, laneHeight: 1.4, rowPadding: 0.6 }}
        interactions={{ drag: true, resize: true, selectSlot: false }}
        columns={[...columns, ...proto.extraColumns]}
        treePanel={{
          width: 592 + proto.extraColumns.reduce((sum, column) => sum + (column.width ?? 0), 0),
          nameColumnWidth: 248,
          minWidth: 360,
          maxWidth: 920
        }}
        i18n={{ labels: { resources: 'Work Items' } }}
        canDropEvent={(update) => {
          const id = Number(update.event.resourceId ?? update.event.id)
          return itemsRef.current.find((row) => row.id === id)?.hasDateFields === true
        }}
        onEventUpdate={handleEventUpdate}
        onEventDoubleClick={(occurrence) =>
          onOpen(Number(occurrence.event.resourceId ?? occurrence.event.id))
        }
        onResourceDoubleClick={({ resource }) => onOpen(Number(resource.id))}
        renderResourceLabel={({ resource }) => {
          const item = byId.get(Number(resource.id))
          return (
            <span
              className="flex min-w-0 items-baseline gap-1.5"
              data-testid={`work-item-${resource.id}`}
              title={item?.title ?? resource.title}
            >
              <span className="text-muted-foreground shrink-0 font-mono text-[12px]">
                #{resource.id}
              </span>
              <span className="truncate font-medium">{item?.title ?? resource.title}</span>
            </span>
          )
        }}
      >
        <GanttNav className="px-4 py-2.5" />
        <GanttView />
      </Gantt>
      {proto.chrome}
    </div>
  )
}
