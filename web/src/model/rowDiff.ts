// How the list of rows changed between two block lists: which rows went away (they leave with an
// animation, in place), and whether rows were added above the old first one (older messages).

import type { Block } from './blocks.ts'

export interface LeavingRow {
  block: Block
  after: string // key of the nearest row before it that stays, '' for the start
}

export interface DrawnRow {
  block: Block
  leaving: boolean
}

export function findLeaving(previous: readonly Block[], current: ReadonlySet<string>): LeavingRow[] {
  let after = ''
  const rows: LeavingRow[] = []
  for (const block of previous) {
    if (current.has(block.key)) after = block.key
    else rows.push({ block, after })
  }
  return rows
}

// How many rows sit above the old first row now; -1 when it is gone (or there was none).
export function prependedCount(previous: readonly Block[], current: readonly Block[]): number {
  const first = previous[0]
  return first ? current.findIndex((block) => block.key === first.key) : -1
}

// The blocks as drawn: the leaving rows stay where they were, behind the row they followed.
export function withLeaving(blocks: readonly Block[], leaving: readonly LeavingRow[]): DrawnRow[] {
  if (leaving.length === 0) return blocks.map((block) => ({ block, leaving: false }))
  const ghostsAfter = (key: string) =>
    leaving.filter((row) => row.after === key).map((row) => ({ block: row.block, leaving: true }))
  return [
    ...ghostsAfter(''),
    ...blocks.flatMap((block) => [{ block, leaving: false }, ...ghostsAfter(block.key)]),
  ]
}
