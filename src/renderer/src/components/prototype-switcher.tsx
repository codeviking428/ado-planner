// PROTOTYPE — throwaway. Hidden in production. Shared by UI prototypes.

import { ChevronLeftIcon, ChevronRightIcon } from 'lucide-react'
import { useEffect, useState } from 'react'

export type PrototypeVariant = {
  key: string
  name: string
}

function readVariant(keys: string[], fallback: string): string {
  const raw = new URLSearchParams(window.location.search).get('variant')
  return raw && keys.includes(raw) ? raw : fallback
}

function writeVariant(key: string): void {
  const url = new URL(window.location.href)
  url.searchParams.set('variant', key)
  window.history.replaceState(null, '', url)
}

export function PrototypeSwitcher({
  variants,
  current,
  onChange
}: {
  variants: PrototypeVariant[]
  current: string
  onChange: (key: string) => void
}) {
  if (import.meta.env.PROD) {
    return null
  }

  const index = Math.max(
    0,
    variants.findIndex((row) => row.key === current)
  )
  const label = variants[index] ? `${variants[index].key} — ${variants[index].name}` : current

  const cycle = (delta: number) => {
    const next = variants[(index + delta + variants.length) % variants.length]
    onChange(next.key)
  }

  return (
    <div
      data-prototype-switcher=""
      className="pointer-events-auto fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-full border border-zinc-700 bg-zinc-950 px-2 py-1.5 text-zinc-50 shadow-lg"
    >
      <button
        type="button"
        aria-label="Previous variant"
        className="flex size-8 items-center justify-center rounded-full hover:bg-zinc-800"
        onClick={() => cycle(-1)}
      >
        <ChevronLeftIcon className="size-4" />
      </button>
      <span className="min-w-52 px-2 text-center text-xs font-medium tabular-nums">{label}</span>
      <button
        type="button"
        aria-label="Next variant"
        className="flex size-8 items-center justify-center rounded-full hover:bg-zinc-800"
        onClick={() => cycle(1)}
      >
        <ChevronRightIcon className="size-4" />
      </button>
    </div>
  )
}

export function usePrototypeVariant(variants: PrototypeVariant[]): {
  variant: string
  setVariant: (key: string) => void
} {
  const keys = variants.map((row) => row.key)
  const fallback = variants[0]?.key ?? 'A'
  const [variant, setVariantState] = useState(() => readVariant(keys, fallback))
  const keysJoin = keys.join(',')

  useEffect(() => {
    const list = keysJoin.split(',').filter(Boolean)
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.tagName === 'SELECT' ||
          target.isContentEditable)
      ) {
        return
      }
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
        event.preventDefault()
        const index = Math.max(0, list.indexOf(variant))
        const delta = event.key === 'ArrowLeft' ? -1 : 1
        const next = list[(index + delta + list.length) % list.length]
        writeVariant(next)
        setVariantState(next)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [keysJoin, variant])

  return {
    variant,
    setVariant: (key: string) => {
      writeVariant(key)
      setVariantState(key)
    }
  }
}
