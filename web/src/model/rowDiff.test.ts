import { describe, expect, it } from 'vitest'
import type { Block } from './blocks.ts'
import { findLeaving, prependedCount, withLeaving } from './rowDiff.ts'

const block = (key: string): Block => ({
  kind: 'markdown',
  key,
  messageId: key,
  streaming: false,
  stale: false,
  text: key,
})
const keys = (rows: { block: Block }[]) => rows.map((row) => row.block.key)

describe('findLeaving', () => {
  it('names the row each removed row followed', () => {
    const rows = findLeaving([block('a'), block('b'), block('c'), block('d')], new Set(['a', 'd']))
    expect(rows.map((row) => [row.block.key, row.after])).toEqual([
      ['b', 'a'],
      ['c', 'a'],
    ])
  })

  it('uses the start for rows removed at the top', () => {
    expect(findLeaving([block('a'), block('b')], new Set(['b']))[0]?.after).toBe('')
  })
})

describe('withLeaving', () => {
  it('keeps the leaving rows in place', () => {
    const leaving = findLeaving([block('a'), block('b'), block('c')], new Set(['a', 'c']))
    const rows = withLeaving([block('a'), block('c'), block('d')], leaving)
    expect(keys(rows)).toEqual(['a', 'b', 'c', 'd'])
    expect(rows.map((row) => row.leaving)).toEqual([false, true, false, false])
  })

  it('draws plain blocks when nothing leaves', () => {
    expect(withLeaving([block('a')], []).map((row) => row.leaving)).toEqual([false])
  })
})

describe('prependedCount', () => {
  it('counts the rows above the old first row', () => {
    expect(prependedCount([block('b'), block('c')], [block('x'), block('y'), block('b'), block('c')])).toBe(2)
    expect(prependedCount([block('b')], [block('b')])).toBe(0)
  })

  it('is -1 when the old first row is gone or there was none', () => {
    expect(prependedCount([block('b')], [block('c')])).toBe(-1)
    expect(prependedCount([], [block('c')])).toBe(-1)
  })
})
