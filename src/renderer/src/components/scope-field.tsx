import { useState } from 'react'
import { Input } from '@/components/ui/input'
import { Spinner } from '@/components/ui/spinner'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from '@/components/ui/select'

const NONE = '__none__'

function matchesQuery(label: string, query: string): boolean {
  const needle = query.trim().toLowerCase()
  return needle.length === 0 || label.toLowerCase().includes(needle)
}

export function ScopeField({
  id,
  label,
  value,
  onChange,
  disabled,
  loading,
  placeholder,
  options,
  allowEmpty = true
}: {
  id: string
  label: string
  value: string
  onChange: (value: string) => void
  disabled?: boolean
  loading?: boolean
  placeholder: string
  options: Array<{ value: string; label: string }>
  allowEmpty?: boolean
}) {
  const [query, setQuery] = useState('')
  const selected = allowEmpty ? value || NONE : value
  const filtered = options.filter((option) => matchesQuery(option.label, query))
  const showEmpty = allowEmpty && matchesQuery(placeholder, query)
  const labelFor = (next: string | null) => {
    if (!next || next === NONE) {
      return placeholder
    }
    return options.find((option) => option.value === next)?.label ?? next
  }

  return (
    <label className="flex min-w-0 flex-col gap-1.5">
      <span className="text-muted-foreground text-[11px] leading-none font-medium tracking-wide uppercase">
        {label}
      </span>
      <span className="flex items-center gap-1.5">
        {loading ? <Spinner className="size-3.5" data-testid={`${id}-loading`} /> : null}
        <Select
          value={selected}
          disabled={disabled}
          onValueChange={(next) => onChange(!next || next === NONE ? '' : next)}
          onOpenChange={(next) => {
            if (!next) {
              setQuery('')
            }
          }}
        >
          <SelectTrigger
            id={id}
            aria-label={label}
            aria-busy={loading}
            data-testid={id}
            className="h-8 max-w-48 min-w-32"
          >
            <SelectValue placeholder={placeholder}>{labelFor}</SelectValue>
          </SelectTrigger>
          <SelectContent alignItemWithTrigger={false} align="start" className="min-w-52">
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
            {showEmpty ? <SelectItem value={NONE}>{placeholder}</SelectItem> : null}
            {filtered.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
            {filtered.length === 0 && !showEmpty ? (
              <p className="text-muted-foreground px-2 py-1.5 text-sm">No matches</p>
            ) : null}
          </SelectContent>
        </Select>
      </span>
    </label>
  )
}
