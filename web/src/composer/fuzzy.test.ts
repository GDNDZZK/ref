import { describe, expect, it } from 'vitest'
import { fuzzyScore } from './fuzzy.ts'

describe('fuzzyScore', () => {
  it('matches subsequences only', () => {
    expect(fuzzyScore('compact', 'cmp')).toBeDefined()
    expect(fuzzyScore('compact', 'cpm')).toBeUndefined()
    expect(fuzzyScore('anything', '')).toBe(0)
  })

  it('ranks a prefix above a scattered match', () => {
    expect(fuzzyScore('undo', 'un')!).toBeGreaterThan(fuzzyScore('running', 'un')!)
  })

  it('ignores case and spaces in the query', () => {
    expect(fuzzyScore('Export', 'e x')).toBeDefined()
  })

  it('scores like the TUI', () => {
    // u: 10 + word start 12; n: 10 + 15 adjacent; prefix 40
    expect(fuzzyScore('undo', 'un')).toBe(10 + 12 + 10 + 15 + 40)
  })
})
