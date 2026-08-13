export const KNOWN_TYPE_COLORS: Record<string, string> = {
  Epic: '#cba6f7',
  Feature: '#89b4fa',
  Story: '#a6e3a1',
  'User Story': '#a6e3a1',
  'Product Backlog Item': '#a6e3a1',
  Issue: '#a6e3a1',
  Requirement: '#a6e3a1',
  Task: '#fab387',
  Bug: '#f38ba8'
}

const FALLBACK = ['#94e2d5', '#f9e2af', '#b4befe', '#f5c2e7', '#74c7ec']

export function colorForType(type: string): string {
  if (KNOWN_TYPE_COLORS[type]) {
    return KNOWN_TYPE_COLORS[type]
  }
  let hash = 0
  for (let i = 0; i < type.length; i += 1) {
    hash = (hash * 31 + type.charCodeAt(i)) >>> 0
  }
  return FALLBACK[hash % FALLBACK.length]
}

export const FLAVORS = ['latte', 'frappe', 'macchiato', 'mocha'] as const
export type Flavor = (typeof FLAVORS)[number]

export function isDarkFlavor(flavor: Flavor): boolean {
  return flavor !== 'latte'
}

export function defaultFlavor(prefersDark: boolean): Flavor {
  return prefersDark ? 'mocha' : 'latte'
}
