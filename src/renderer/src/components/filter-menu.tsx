import { ChevronDownIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
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
  const hiddenCount = hidden.length
  return (
    <DropdownMenu>
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
        <DropdownMenuGroup>
          <DropdownMenuLabel>Show {label.toLowerCase()}</DropdownMenuLabel>
          {items.map((item) => {
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
