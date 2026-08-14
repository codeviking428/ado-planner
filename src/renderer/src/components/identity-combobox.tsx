import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { showErrorToast } from '@/lib/error-toast'
import type { IdentityValue } from '@shared/types'

type IdentityComboboxProps = {
  org: string
  value: IdentityValue | null
  onChange: (value: IdentityValue | null) => void
  disabled?: boolean
}

export function IdentityCombobox({ org, value, onChange, disabled }: IdentityComboboxProps) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [options, setOptions] = useState<IdentityValue[]>([])

  useEffect(() => {
    if (!open || query.trim().length < 1) {
      return
    }
    const handle = window.setTimeout(() => {
      void window.planner.ado
        .identities(org, query.trim())
        .then(setOptions)
        .catch((error: unknown) => {
          setOptions([])
          showErrorToast(error, 'Could not search identities')
        })
    }, 200)
    return () => window.clearTimeout(handle)
  }, [open, org, query])

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        nativeButton={false}
        render={
          <Button
            type="button"
            variant="outline"
            disabled={disabled}
            className="w-full justify-start font-normal"
            data-testid="identity-combobox"
          >
            {value?.displayName || value?.uniqueName || 'Unassigned'}
          </Button>
        }
      />
      <PopoverContent className="w-80 p-2">
        <Input
          autoFocus
          placeholder="Search identities"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <button
          type="button"
          className="hover:bg-muted mt-2 w-full rounded-md px-2 py-1.5 text-left text-sm"
          onClick={() => {
            onChange(null)
            setOpen(false)
          }}
        >
          Unassigned
        </button>
        <ul className="mt-1 max-h-48 overflow-auto">
          {options.map((option) => (
            <li key={option.uniqueName}>
              <button
                type="button"
                className="hover:bg-muted w-full rounded-md px-2 py-1.5 text-left text-sm"
                onClick={() => {
                  onChange(option)
                  setOpen(false)
                }}
              >
                {option.displayName}
                <span className="text-muted-foreground ml-2 text-xs">{option.uniqueName}</span>
              </button>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  )
}
