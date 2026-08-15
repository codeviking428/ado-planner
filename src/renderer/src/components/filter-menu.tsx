import { useState } from 'react'
import { ChevronDownIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from '@/components/ui/dropdown-menu'

function matchesQuery(label: string, query: string): boolean {
  const needle = query.trim().toLowerCase()
  return needle.length === 0 || label.toLowerCase().includes(needle)
}

export function FilterMenu({
  id,
  label,
  items,
  hidden,
  onChange
}: {
  id: string
  label: string
  items: string[]
  hidden: string[]
  onChange: (hidden: string[]) => void
}) {
  const [query, setQuery] = useState('')
  const hiddenCount = hidden.length
  const filtered = items.filter((item) => matchesQuery(item, query))

  return (
    <DropdownMenu
      onOpenChange={(open) => {
        if (!open) {
          setQuery('')
        }
      }}
    >
      <DropdownMenuTrigger
        render={
          <Button
            variant="outline"
            data-testid={id}
            aria-label={
              hiddenCount ? `${label}, ${hiddenCount} hidden` : `Filter ${label.toLowerCase()}`
            }
          />
        }
      >
        {label}
        {hiddenCount > 0 ? (
          <span className="text-muted-foreground tabular-nums">{hiddenCount} hidden</span>
        ) : null}
        <ChevronDownIcon className="size-3.5 opacity-60" aria-hidden />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-64">
        <div className="bg-popover sticky top-0 z-10 p-1">
          <Input
            autoFocus
            data-testid={`${id}-search`}
            placeholder={`Search ${label.toLowerCase()}`}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
          />
        </div>
        <DropdownMenuGroup>
          <DropdownMenuLabel>Show {label.toLowerCase()}</DropdownMenuLabel>
          {filtered.map((item) => {
            const visible = !hidden.includes(item)
            return (
              <DropdownMenuCheckboxItem
                key={item}
                checked={visible}
                onCheckedChange={(checked) =>
                  onChange(checked ? hidden.filter((row) => row !== item) : [...hidden, item])
                }
              >
                {item}
              </DropdownMenuCheckboxItem>
            )
          })}
          {filtered.length === 0 ? (
            <p className="text-muted-foreground px-2 py-1.5 text-sm">No matches</p>
          ) : null}
        </DropdownMenuGroup>
        {items.length > 0 ? (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => onChange([])}>Show all</DropdownMenuItem>
            <DropdownMenuItem onClick={() => onChange(items)}>Hide all</DropdownMenuItem>
          </>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
