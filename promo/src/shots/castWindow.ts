import type { Rect } from '../components/appFrame.ts'
import { pendingFrame } from '../components/pendingFrame.ts'
import { terminalView, type TerminalViewOptions } from '../components/terminalView.ts'
import { findCast } from '../engine/casts.ts'
import type { Part } from '../engine/part.ts'

export type CastWindowOptions = Omit<TerminalViewOptions, 'cast'> & {
  // Recorded cast name in assets/casts.
  name: string
  // Size of the placeholder while the cast is not recorded yet.
  pendingSize?: { width: number; height: number }
}

const DEFAULT_PENDING_SIZE = { width: 1400, height: 760 }

// A terminal replaying a recorded cast, or a labelled placeholder until it is recorded.
export function castWindow(options: CastWindowOptions): Part {
  const url = findCast(options.name)
  if (url) return terminalView({ ...options, cast: url })
  const size = options.pendingSize ?? DEFAULT_PENDING_SIZE
  const rect: Rect = { x: options.x, y: options.y, ...size }
  return pendingFrame({ names: [options.name], title: options.title, rect, from: options.from, to: options.to })
}
