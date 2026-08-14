import * as React from 'react'
import { defaultFlavor, FLAVORS, isDarkFlavor, type Flavor } from '@shared/flavor'

type FlavorState = {
  flavor: Flavor
  setFlavor: (flavor: Flavor) => void
}

const FlavorContext = React.createContext<FlavorState | undefined>(undefined)
const STORAGE_KEY = 'ado-planner.flavor'

function readStoredFlavor(): Flavor | null {
  const stored = localStorage.getItem(STORAGE_KEY)
  return FLAVORS.includes(stored as Flavor) ? (stored as Flavor) : null
}

function applyFlavor(flavor: Flavor): void {
  const root = document.documentElement
  root.classList.remove('light', 'dark')
  root.classList.add(isDarkFlavor(flavor) ? 'dark' : 'light')
  root.dataset.flavor = flavor
}

export function FlavorProvider({ children }: { children: React.ReactNode }) {
  const [flavor, setFlavorState] = React.useState<Flavor>(() => {
    return readStoredFlavor() ?? defaultFlavor()
  })

  const setFlavor = React.useCallback((next: Flavor) => {
    localStorage.setItem(STORAGE_KEY, next)
    setFlavorState(next)
  }, [])

  React.useEffect(() => {
    applyFlavor(flavor)
  }, [flavor])

  const value = React.useMemo(() => ({ flavor, setFlavor }), [flavor, setFlavor])
  return <FlavorContext.Provider value={value}>{children}</FlavorContext.Provider>
}

export function useFlavor(): FlavorState {
  const ctx = React.useContext(FlavorContext)
  if (!ctx) {
    throw new Error('useFlavor must be used within FlavorProvider')
  }
  return ctx
}

export function useTheme() {
  const { flavor } = useFlavor()
  return { theme: isDarkFlavor(flavor) ? 'dark' : 'light' }
}
