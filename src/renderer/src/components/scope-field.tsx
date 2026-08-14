import { Spinner } from '@/components/ui/spinner'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from '@/components/ui/select'

const NONE = '__none__'

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
  const selected = allowEmpty ? value || NONE : value
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
          <SelectContent alignItemWithTrigger={false} align="start" className="min-w-44">
            {allowEmpty ? <SelectItem value={NONE}>{placeholder}</SelectItem> : null}
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </span>
    </label>
  )
}
