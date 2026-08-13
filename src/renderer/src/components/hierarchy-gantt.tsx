import { useMemo, useRef } from 'react'
import { format } from 'date-fns'
import { toast } from 'sonner'
import { Gantt, type GanttColumn } from '@/components/reui/gantt/gantt'
import { GanttNav } from '@/components/reui/gantt/gantt-nav'
import type {
  GanttEvent,
  GanttProposedUpdate,
  GanttResource
} from '@/components/reui/gantt/gantt-types'
import { GanttView } from '@/components/reui/gantt/gantt-view'
import { colorForType } from '@shared/flavor'
import {
  fromGanttExclusiveEnd,
  isUnscheduled,
  isoDateOnly,
  toGanttInclusiveTarget
} from '@shared/dates'
import type { ScopeSelection, WorkItemNode } from '@shared/types'

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
  onItemsChange,
  onOpen
}: {
  scope: ScopeSelection
  items: WorkItemNode[]
  onItemsChange: (items: WorkItemNode[]) => void
  onOpen: (id: number) => void
}) {
  const itemsRef = useRef(items)
  itemsRef.current = items
  const byId = useMemo(() => new Map(items.map((item) => [item.id, item])), [items])
  const resources = useMemo(() => toResources(items), [items])
  const events = useMemo(() => toEvents(items), [items])

  const columns: GanttColumn[] = useMemo(
    () => [
      {
        id: 'type',
        title: 'Type',
        width: 88,
        render: ({ resource }) => byId.get(Number(resource.id))?.type ?? ''
      },
      {
        id: 'dates',
        title: 'Start / Target',
        width: 220,
        render: ({ resource }) => {
          const item = byId.get(Number(resource.id))
          if (!item) {
            return null
          }
          if (!item.hasDateFields) {
            return 'No Start/Target on type'
          }
          if (isUnscheduled(item)) {
            return 'Unscheduled'
          }
          return `${format(new Date(item.startDate as string), 'yyyy-MM-dd')} → ${format(new Date(item.targetDate as string), 'yyyy-MM-dd')}`
        }
      }
    ],
    [byId]
  )

  const handleEventUpdate = (update: GanttProposedUpdate) => {
    const id = Number(update.event.resourceId ?? update.event.id)
    const item = itemsRef.current.find((row) => row.id === id)
    if (!item) {
      return false
    }
    if (!item.hasDateFields) {
      toast.error(`#${item.id} ${item.type} has no Start Date / Target Date`)
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
        toast.error(error instanceof Error ? error.message : 'PATCH failed')
      })
    return true
  }

  if (items.length === 0) {
    return (
      <div
        className="text-muted-foreground flex h-full items-center justify-center text-sm"
        data-testid="empty-gantt"
      >
        No Work Items in this Team.
      </div>
    )
  }

  return (
    <Gantt
      events={events}
      resources={resources}
      defaultScale="month"
      className="h-full text-xs"
      summaryBars
      parentScheduling={false}
      rowCheckboxes={false}
      dragCreate={false}
      displayScheduleHint={false}
      interactions={{ drag: true, resize: true, selectSlot: false }}
      columns={columns}
      treePanel={{ width: 420, nameColumnWidth: 200 }}
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
          >
            <span className="text-muted-foreground font-mono">#{resource.id}</span>
            <span className="truncate">{item?.title ?? resource.title}</span>
          </span>
        )
      }}
    >
      <GanttNav />
      <GanttView />
    </Gantt>
  )
}
