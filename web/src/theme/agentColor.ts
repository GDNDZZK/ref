// The color of an agent (Theme.agentColor in Theme.qml). `spec` is the agent's configured color:
// #rrggbb, #rgb or a name from the TUI's configuration. Anything else takes an entry of the
// palette by the agent's position in the agent list.
//
// Returns a CSS color: `var(--agent-blue)` for names and palette entries (so the dark theme gets
// its own shade), the hex string as written otherwise.

import { agentPalette } from './generated/theme.ts'

// Names as in Theme.qml's `named` table; the values are the --agent-* variables in theme.css.
const NAMED_COLORS: Record<string, string> = {
  primary: 'agent-orange',
  orange: 'agent-orange',
  secondary: 'agent-blue',
  blue: 'agent-blue',
  accent: 'agent-purple',
  purple: 'agent-purple',
  error: 'agent-red',
  red: 'agent-red',
  warning: 'agent-yellow',
  yellow: 'agent-yellow',
  success: 'agent-green',
  green: 'agent-green',
  info: 'agent-cyan',
  cyan: 'agent-cyan',
}

const HEX_COLOR = /^#([0-9a-f]{3}|[0-9a-f]{6})$/

export function agentColor(spec: string, index: number): string {
  const key = spec.trim().toLowerCase()
  const named = Object.hasOwn(NAMED_COLORS, key) ? NAMED_COLORS[key] : undefined
  if (named) return `var(--${named})`
  if (HEX_COLOR.test(key)) return key
  return `var(--agent-palette-${Math.abs(index) % agentPalette.length})`
}

const HASH_MULTIPLIER = 31

function hashString(value: string): number {
  return [...value].reduce((hash, char) => (hash * HASH_MULTIPLIER + char.charCodeAt(0)) >>> 0, 0)
}

// A palette entry from the agent palette hashed deterministically from any string key (e.g. root session id).
export function paletteColorFor(key: string): string {
  return `var(--agent-palette-${hashString(key) % agentPalette.length})`
}

export const ROOT_PALETTE = [
  'var(--agent-blue)',
  'var(--agent-purple)',
  'var(--agent-green)',
  'var(--agent-orange)',
  'var(--agent-cyan)',
  'var(--agent-yellow)',
  'var(--agent-red)',
] as const

// Conflict-free deterministic tint color assignment for concurrent root/parent sessions.
export function assignRootColors(rootIds: Iterable<string>): Map<string, string> {
  const unique = [...new Set(rootIds)].filter((id) => id !== '')
  unique.sort()

  const size = ROOT_PALETTE.length
  const used = new Array<number>(size).fill(0)
  const assigned = new Map<string, string>()

  for (const rootId of unique) {
    const preferred = hashString(rootId) % size
    const minUsed = Math.min(...used)
    let chosen = preferred
    for (let offset = 0; offset < size; offset++) {
      const slot = (preferred + offset) % size
      if (used[slot] === minUsed) {
        chosen = slot
        break
      }
    }
    used[chosen] = (used[chosen] ?? 0) + 1
    const color = ROOT_PALETTE[chosen]
    if (color !== undefined) assigned.set(rootId, color)
  }

  return assigned
}

// Position in the agent list, or a hash of the name for an agent the list does not have.
function paletteIndex(agents: readonly { name: string }[], name: string): number {
  const position = agents.findIndex((agent) => agent.name === name)
  return position >= 0 ? position : hashString(name)
}

// The color of the agent called `name`, given the server's agent list.
export function agentColorByName(
  agents: readonly { name: string; color: string | null }[],
  name: string,
): string {
  const spec = agents.find((agent) => agent.name === name)?.color ?? ''
  return agentColor(spec, paletteIndex(agents, name))
}
