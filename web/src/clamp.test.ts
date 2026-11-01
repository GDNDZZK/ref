import { describe, expect, it } from 'vitest'
import { clamp } from './clamp.ts'

describe('clamp', () => {
  it('holds a value within the range', () => {
    expect(clamp(5, 0, 10)).toBe(5)
    expect(clamp(-3, 0, 10)).toBe(0)
    expect(clamp(12, 0, 10)).toBe(10)
  })

  it('gives the lower bound when the range is empty', () => {
    expect(clamp(5, 0, -2)).toBe(0)
  })
})
