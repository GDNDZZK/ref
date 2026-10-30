import { describe, expect, it } from 'vitest'
import { formatClock, formatDuration, formatElapsed, formatTokens, secondsUntil } from './format.ts'

describe('format', () => {
  it('writes durations by size', () => {
    expect(formatDuration(850)).toBe('850ms')
    expect(formatDuration(1400)).toBe('1.4s')
    expect(formatDuration(12_000)).toBe('12s')
    expect(formatDuration(123_000)).toBe('2m 03s')
    expect(formatDuration(3_720_000)).toBe('1h 02m')
    expect(formatDuration(-5)).toBe('0ms')
  })

  it('counts elapsed time in whole seconds', () => {
    expect(formatElapsed(3999)).toBe('3s')
    expect(formatElapsed(65_000)).toBe('1m 05s')
  })

  it('shortens token counts', () => {
    expect(formatTokens(950)).toBe('950')
    expect(formatTokens(12_345)).toBe('12.3k')
    expect(formatTokens(999_960)).toBe('1.0M')
  })

  it('pads the clock', () => {
    expect(formatClock(new Date(2026, 0, 2, 3, 4, 5).getTime())).toBe('03:04:05')
  })

  it('rounds the seconds left up', () => {
    expect(secondsUntil(10_001, 10_000)).toBe(1)
    expect(secondsUntil(9000, 10_000)).toBe(0)
    expect(secondsUntil(14_000, 10_000)).toBe(4)
  })
})
