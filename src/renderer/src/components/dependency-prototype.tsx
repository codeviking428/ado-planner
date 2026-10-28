// PROTOTYPE — throwaway. Three variants of Dependency arrows on the existing Gantt, switchable via ?variant=.
// A drag connector · B two-click · C Monday-like column (arrows display-only). In-memory links only.

import { useEffect, useLayoutEffect, useMemo, useState, type ReactNode } from 'react'
import { XIcon } from 'lucide-react'
import { toast } from 'sonner'
import type { GanttColumn } from '@/components/reui/gantt/gantt'
import {
  PrototypeSwitcher,
  usePrototypeVariant,
  type PrototypeVariant
} from '@/components/prototype-switcher'
import type { WorkItemNode } from '@shared/types'

export type DependencyLink = {
  predecessorId: number
  successorId: number
}

const VARIANTS: PrototypeVariant[] = [
  { key: 'A', name: 'Drag connector' },
  { key: 'B', name: 'Two-click' },
  { key: 'C', name: 'Column picker' }
]

type BarGeom = { id: number; left: number; right: number; top: number; bottom: number }

function scheduledIds(items: WorkItemNode[]): number[] {
  return items
    .filter((item) => item.hasDateFields && item.startDate && item.targetDate)
    .map((item) => item.id)
}

function seedLinks(items: WorkItemNode[]): DependencyLink[] {
  const ids = scheduledIds(items).slice(0, 4)
  const links: DependencyLink[] = []
  for (let i = 0; i < ids.length - 1; i++) {
    links.push({ predecessorId: ids[i], successorId: ids[i + 1] })
  }
  return links
}

function wouldCycle(links: DependencyLink[], predecessorId: number, successorId: number): boolean {
  const stack = [successorId]
  const seen = new Set<number>()
  while (stack.length > 0) {
    const current = stack.pop() as number
    if (current === predecessorId) {
      return true
    }
    if (seen.has(current)) {
      continue
    }
    seen.add(current)
    for (const link of links) {
      if (link.predecessorId === current) {
        stack.push(link.successorId)
      }
    }
  }
  return false
}

function addLink(
  links: DependencyLink[],
  predecessorId: number,
  successorId: number
): DependencyLink[] | null {
  if (predecessorId === successorId) {
    toast.message('A Work Item cannot depend on itself')
    return null
  }
  if (
    links.some((link) => link.predecessorId === predecessorId && link.successorId === successorId)
  ) {
    toast.message('That Dependency already exists')
    return null
  }
  if (wouldCycle(links, predecessorId, successorId)) {
    toast.error('That Dependency would cycle')
    return null
  }
  return [...links, { predecessorId, successorId }]
}

function geomKey(geom: { pane: DOMRect | null; bars: BarGeom[] }): string {
  if (!geom.pane) {
    return ''
  }
  return [
    Math.round(geom.pane.left),
    Math.round(geom.pane.top),
    Math.round(geom.pane.width),
    Math.round(geom.pane.height),
    ...geom.bars.map(
      (bar) =>
        `${bar.id}:${Math.round(bar.left)}:${Math.round(bar.right)}:${Math.round(bar.top)}:${Math.round(bar.bottom)}`
    )
  ].join('|')
}

function measureBars(): { pane: DOMRect | null; bars: BarGeom[] } {
  const pane = document.querySelector<HTMLElement>('[data-slot=gantt-timeline-pane]')
  if (!pane) {
    return { pane: null, bars: [] }
  }
  const paneRect = pane.getBoundingClientRect()
  const bars: BarGeom[] = []
  for (const row of pane.querySelectorAll<HTMLElement>('[data-gantt-row]')) {
    const id = Number(row.getAttribute('data-gantt-row-id'))
    const bar = row.querySelector<HTMLElement>('[data-slot=gantt-bar]')
    if (!Number.isFinite(id) || !bar) {
      continue
    }
    const rect = bar.getBoundingClientRect()
    bars.push({
      id,
      left: rect.left - paneRect.left,
      right: rect.right - paneRect.left,
      top: rect.top - paneRect.top,
      bottom: rect.bottom - paneRect.top
    })
  }
  return { pane: paneRect, bars }
}

function barAtPoint(bars: BarGeom[], x: number, y: number): BarGeom | null {
  return (
    bars.find((bar) => x >= bar.left && x <= bar.right && y >= bar.top && y <= bar.bottom) ?? null
  )
}

function ArrowSvg({
  links,
  bars,
  show,
  hit,
  onDelete,
  draft
}: {
  links: DependencyLink[]
  bars: BarGeom[]
  show: boolean
  hit: boolean
  onDelete?: (link: DependencyLink) => void
  draft?: { x1: number; y1: number; x2: number; y2: number } | null
}) {
  if (!show) {
    return null
  }
  const byId = new Map(bars.map((bar) => [bar.id, bar]))
  return (
    <svg className="pointer-events-none absolute inset-0 size-full overflow-visible">
      <defs>
        <marker
          id="dep-arrow"
          viewBox="0 0 10 10"
          refX="8"
          refY="5"
          markerWidth="7"
          markerHeight="7"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" className="fill-foreground/80" />
        </marker>
      </defs>
      {links.map((link) => {
        const from = byId.get(link.predecessorId)
        const to = byId.get(link.successorId)
        if (!from || !to) {
          return null
        }
        const x1 = from.right
        const y1 = (from.top + from.bottom) / 2
        const x2 = to.left
        const y2 = (to.top + to.bottom) / 2
        const midX = (x1 + x2) / 2
        const midY = (y1 + y2) / 2
        const d = `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`
        return (
          <g key={`${link.predecessorId}-${link.successorId}`}>
            <path
              d={d}
              fill="none"
              className="stroke-foreground/70"
              strokeWidth={2}
              markerEnd="url(#dep-arrow)"
            />
            {hit ? (
              <>
                <path
                  d={d}
                  fill="none"
                  stroke="transparent"
                  strokeWidth={14}
                  className="pointer-events-auto cursor-pointer"
                  onClick={(event) => {
                    event.stopPropagation()
                    onDelete?.(link)
                  }}
                >
                  <title>Delete Dependency</title>
                </path>
                {onDelete ? (
                  <g
                    className="pointer-events-auto cursor-pointer"
                    onClick={(event) => {
                      event.stopPropagation()
                      onDelete(link)
                    }}
                  >
                    <circle
                      cx={midX}
                      cy={midY}
                      r={9}
                      className="fill-background stroke-foreground/50"
                    />
                    <text
                      x={midX}
                      y={midY + 3.5}
                      textAnchor="middle"
                      className="fill-foreground text-[10px]"
                    >
                      ×
                    </text>
                  </g>
                ) : null}
              </>
            ) : null}
          </g>
        )
      })}
      {draft ? (
        <path
          d={`M ${draft.x1} ${draft.y1} L ${draft.x2} ${draft.y2}`}
          fill="none"
          className="stroke-foreground/50"
          strokeWidth={2}
          strokeDasharray="4 3"
          markerEnd="url(#dep-arrow)"
        />
      ) : null}
    </svg>
  )
}

function ConnectorHandles({
  bars,
  onBegin
}: {
  bars: BarGeom[]
  onBegin: (id: number, event: React.PointerEvent<HTMLButtonElement>) => void
}) {
  return (
    <>
      {bars.map((bar) => (
        <button
          key={bar.id}
          type="button"
          aria-label={`Drag a Dependency from #${bar.id}`}
          data-prototype-handle={bar.id}
          className="bg-foreground border-background pointer-events-auto absolute z-20 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2"
          style={{ left: bar.right, top: (bar.top + bar.bottom) / 2 }}
          onPointerDown={(event) => {
            event.preventDefault()
            event.stopPropagation()
            onBegin(bar.id, event)
          }}
        />
      ))}
    </>
  )
}

export function useDependencyPrototype(items: WorkItemNode[]): {
  extraColumns: GanttColumn[]
  chrome: ReactNode
} {
  const { variant, setVariant } = usePrototypeVariant(VARIANTS)
  const [links, setLinks] = useState<DependencyLink[]>(() => seedLinks(items))
  const [showArrows, setShowArrows] = useState(true)
  const [pending, setPending] = useState<number | null>(null)
  const [draft, setDraft] = useState<{
    fromId: number
    x1: number
    y1: number
    x2: number
    y2: number
  } | null>(null)
  const [geom, setGeom] = useState<{ pane: DOMRect | null; bars: BarGeom[] }>({
    pane: null,
    bars: []
  })

  const itemIds = useMemo(() => items.map((item) => item.id).join(','), [items])
  useEffect(() => {
    setLinks(seedLinks(items))
    setPending(null)
    setDraft(null)
    // Seed once when the forest identity changes, not on every overlay tick.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [itemIds])

  useLayoutEffect(() => {
    let frame = 0
    let last = ''
    const tick = () => {
      const next = measureBars()
      const key = geomKey(next)
      if (key !== last) {
        last = key
        setGeom(next)
      }
      frame = window.requestAnimationFrame(tick)
    }
    frame = window.requestAnimationFrame(tick)
    return () => window.cancelAnimationFrame(frame)
  }, [])

  const tryAdd = (predecessorId: number, successorId: number) => {
    setLinks((current) => {
      const next = addLink(current, predecessorId, successorId)
      return next ?? current
    })
  }

  const remove = (link: DependencyLink) => {
    setLinks((current) =>
      current.filter(
        (row) => !(row.predecessorId === link.predecessorId && row.successorId === link.successorId)
      )
    )
  }

  useEffect(() => {
    if (variant !== 'B') {
      setPending(null)
      return
    }
    const onClick = (event: MouseEvent) => {
      const bar = (event.target as HTMLElement | null)?.closest?.('[data-slot=gantt-bar]')
      const row = bar?.closest?.('[data-gantt-row-id]')
      const id = Number(row?.getAttribute('data-gantt-row-id'))
      if (!bar || !Number.isFinite(id)) {
        return
      }
      event.stopPropagation()
      setPending((current) => {
        if (current === null) {
          toast.message(`Predecessor #${id} — click a successor`)
          return id
        }
        tryAdd(current, id)
        return null
      })
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setPending(null)
      }
    }
    document.addEventListener('click', onClick, true)
    window.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('click', onClick, true)
      window.removeEventListener('keydown', onKey)
    }
  }, [variant])

  useEffect(() => {
    if (!draft) {
      return
    }
    const onMove = (event: PointerEvent) => {
      const pane = document.querySelector<HTMLElement>('[data-slot=gantt-timeline-pane]')
      if (!pane) {
        return
      }
      const rect = pane.getBoundingClientRect()
      setDraft((current) =>
        current
          ? { ...current, x2: event.clientX - rect.left, y2: event.clientY - rect.top }
          : current
      )
    }
    const onUp = (event: PointerEvent) => {
      const pane = document.querySelector<HTMLElement>('[data-slot=gantt-timeline-pane]')
      const fromId = draft.fromId
      setDraft(null)
      if (!pane) {
        return
      }
      const rect = pane.getBoundingClientRect()
      const { bars } = measureBars()
      const hit = barAtPoint(bars, event.clientX - rect.left, event.clientY - rect.top)
      if (hit) {
        tryAdd(fromId, hit.id)
      }
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp, { once: true })
    return () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
    }
  }, [draft])

  const extraColumns: GanttColumn[] =
    variant === 'C'
      ? [
          {
            id: 'prototype-deps',
            title: 'Dependencies',
            width: 168,
            className: 'min-w-0',
            render: ({ resource }) => {
              const id = Number(resource.id)
              const preds = links.filter((link) => link.successorId === id)
              const options = items.filter((item) => item.id !== id)
              return (
                <div className="flex min-w-0 flex-col gap-0.5 py-0.5" data-prototype-column="">
                  {preds.map((link) => (
                    <span
                      key={link.predecessorId}
                      className="flex items-center justify-between gap-1 font-mono text-[11px]"
                    >
                      #{link.predecessorId}
                      <button
                        type="button"
                        aria-label={`Remove predecessor #${link.predecessorId}`}
                        className="text-muted-foreground hover:text-foreground"
                        onClick={() => remove(link)}
                      >
                        <XIcon className="size-3" />
                      </button>
                    </span>
                  ))}
                  <select
                    className="bg-background max-w-full truncate rounded border px-1 py-0.5 text-[11px]"
                    defaultValue=""
                    onChange={(event) => {
                      const predecessorId = Number(event.target.value)
                      event.target.value = ''
                      if (Number.isFinite(predecessorId) && predecessorId > 0) {
                        tryAdd(predecessorId, id)
                      }
                    }}
                  >
                    <option value="">+ predecessor</option>
                    {options.map((item) => (
                      <option key={item.id} value={item.id}>
                        #{item.id} {item.title}
                      </option>
                    ))}
                  </select>
                </div>
              )
            }
          }
        ]
      : []

  const overlay = geom.pane ? (
    <div
      data-prototype-overlay=""
      className="pointer-events-none fixed z-30"
      style={{
        left: geom.pane.left,
        top: geom.pane.top,
        width: geom.pane.width,
        height: geom.pane.height
      }}
    >
      <ArrowSvg
        links={links}
        bars={geom.bars}
        show={showArrows}
        hit={variant !== 'C'}
        onDelete={variant === 'C' ? undefined : remove}
        draft={draft ? { x1: draft.x1, y1: draft.y1, x2: draft.x2, y2: draft.y2 } : null}
      />
      {variant === 'A' ? (
        <ConnectorHandles
          bars={geom.bars}
          onBegin={(id, event) => {
            const pane = document.querySelector<HTMLElement>('[data-slot=gantt-timeline-pane]')
            if (!pane) {
              return
            }
            const rect = pane.getBoundingClientRect()
            const from = geom.bars.find((bar) => bar.id === id)
            if (!from) {
              return
            }
            ;(event.target as HTMLElement).setPointerCapture?.(event.pointerId)
            setDraft({
              fromId: id,
              x1: from.right,
              y1: (from.top + from.bottom) / 2,
              x2: event.clientX - rect.left,
              y2: event.clientY - rect.top
            })
          }}
        />
      ) : null}
      {variant === 'B' && pending !== null
        ? geom.bars
            .filter((bar) => bar.id === pending)
            .map((bar) => (
              <div
                key={bar.id}
                className="border-foreground pointer-events-none absolute rounded-sm border-2"
                style={{
                  left: bar.left,
                  top: bar.top,
                  width: bar.right - bar.left,
                  height: bar.bottom - bar.top
                }}
              />
            ))
        : null}
    </div>
  ) : null

  const chrome = (
    <>
      {overlay}
      <aside
        data-prototype-state=""
        className="pointer-events-auto fixed top-3 right-3 z-50 max-w-sm rounded-lg border border-zinc-700 bg-zinc-950/90 p-3 font-mono text-[11px] text-zinc-50 shadow-lg"
      >
        <p className="mb-1 font-sans text-[10px] tracking-wide text-zinc-400 uppercase">
          PROTOTYPE state
        </p>
        <label className="mb-2 flex items-center gap-2 font-sans text-xs">
          <input
            type="checkbox"
            checked={showArrows}
            onChange={(event) => setShowArrows(event.target.checked)}
          />
          Show arrows
        </label>
        {variant === 'B' ? (
          <p className="mb-2 font-sans text-xs text-zinc-300">
            {pending === null
              ? 'Click a Gantt bar for the predecessor, then a successor. Esc cancels.'
              : `Pending predecessor #${pending}`}
          </p>
        ) : null}
        {variant === 'A' ? (
          <p className="mb-2 font-sans text-xs text-zinc-300">
            Drag the dot on a bar’s right edge onto another bar. Click an arrow to delete.
          </p>
        ) : null}
        {variant === 'C' ? (
          <p className="mb-2 font-sans text-xs text-zinc-300">
            Arrows are display-only. Add/remove predecessors in the Dependencies column.
          </p>
        ) : null}
        <pre className="max-h-40 overflow-auto whitespace-pre-wrap">
          {JSON.stringify({ variant, showArrows, pending, links }, null, 2)}
        </pre>
      </aside>
      <PrototypeSwitcher variants={VARIANTS} current={variant} onChange={setVariant} />
    </>
  )

  return { extraColumns, chrome }
}
