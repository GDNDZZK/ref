import { describe, expect, it } from 'vitest'
import { outElastic, plainEasings } from './easing.ts'
import type { EasingFn } from './easing.ts'
import { DEFAULT_TOLERANCE, evaluateLinear, linearPoints, toCssLinear } from './linearCss.ts'

const CHECKED_PROGRESS_STEPS = 2000
const ROUNDING_SLACK = 0.0005 // the printed values have 5 decimals, positions 2 percent decimals

function maxError(easing: EasingFn): number {
  const points = linearPoints(easing)
  let worst = 0
  for (let i = 0; i <= CHECKED_PROGRESS_STEPS; i++) {
    const progress = i / CHECKED_PROGRESS_STEPS
    worst = Math.max(worst, Math.abs(evaluateLinear(points, progress) - easing(progress)))
  }
  return worst
}

const curves: [string, EasingFn][] = [
  ...Object.entries(plainEasings),
  ['flourish spring', outElastic(2, 0.35)],
  ['flourish entry spring', outElastic(1.4, 0.35)],
]

describe('linear()', () => {
  it.each(curves)('%s stays within tolerance of the curve', (_name, easing) => {
    expect(maxError(easing)).toBeLessThan(DEFAULT_TOLERANCE + ROUNDING_SLACK)
  })

  it('starts at 0 and ends at 1', () => {
    const points = linearPoints(plainEasings.outExpo)
    expect(points[0]).toEqual({ progress: 0, value: 0 })
    expect(points.at(-1)).toEqual({ progress: 1, value: 1 })
  })

  it('needs only two points for a straight line', () => {
    expect(linearPoints(plainEasings.linear)).toHaveLength(2)
  })

  it('prints positions for inner points only and keeps overshoot values', () => {
    const css = toCssLinear(linearPoints(plainEasings.outBack))
    expect(css).toMatch(/^linear\(0, 0\.\d+ \d+(\.\d+)?%/)
    expect(css).toMatch(/, 1\)$/)
    const values = [...css.matchAll(/(?:^|[(,] )(\d+\.\d+)(?= \d)/g)].map((m) => Number(m[1]))
    expect(Math.max(...values)).toBeGreaterThan(1)
  })
})
