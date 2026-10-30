import { describe, expect, it } from 'vitest'
import { fuzzyScore } from './fuzzy.ts'

describe('fuzzyScore', () => {
  it('matches subsequences only', () => {
    // c: 10 + word start 12; m (2 after c): 10 - 2/2; p (adjacent to m): 10 + 15; no prefix bonus
    expect(fuzzyScore('compact', 'cmp')).toBe(10 + 12 + (10 - 1) + (10 + 15))
    expect(fuzzyScore('compact', 'cpm')).toBeUndefined()
    expect(fuzzyScore('anything', '')).toBe(0)
  })

  it('ranks a prefix above a scattered match', () => {
    expect(fuzzyScore('undo', 'un')!).toBeGreaterThan(fuzzyScore('running', 'un')!)
  })

  it('ignores case and spaces in the query', () => {
    // e: 10 + word start 12; x (adjacent): 10 + 15. The space is skipped when matching, but the
    // prefix bonus compares the query with its space ("e x" is not a prefix of "export").
    expect(fuzzyScore('Export', 'e x')).toBe(10 + 12 + 10 + 15)
  })

  it('scores like the TUI', () => {
    // u: 10 + word start 12; n: 10 + 15 adjacent; prefix 40
    expect(fuzzyScore('undo', 'un')).toBe(10 + 12 + 10 + 15 + 40)
  })
})
