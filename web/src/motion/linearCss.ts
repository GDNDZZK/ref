// Turns an easing function into a CSS `linear()` timing function.
//
//   f(t):  ____..----''''     sampled at the fewest points whose straight segments
//                             stay within `tolerance` of the curve
//
// Positions sit on a grid of 1/POSITION_STEPS so the printed percentages are exact.

import type { EasingFn } from './easing.ts'

export interface LinearPoint {
  progress: number // 0..1, on the position grid
  value: number
}

const POSITION_STEPS = 10000
const VALUE_DECIMALS = 5
const PERCENT_DECIMALS = 2
const CHECKED_FRACTIONS = [0.25, 0.5, 0.75] // inside a segment, where the curve is compared to the chord
export const DEFAULT_TOLERANCE = 0.001

function roundTo(value: number, decimals: number): number {
  const scale = 10 ** decimals
  return Math.round(value * scale) / scale
}

export function linearPoints(easing: EasingFn, tolerance: number = DEFAULT_TOLERANCE): LinearPoint[] {
  const pointAt = (step: number): LinearPoint => {
    const progress = step / POSITION_STEPS
    return { progress, value: roundTo(easing(progress), VALUE_DECIMALS) }
  }

  const points: LinearPoint[] = [pointAt(0)]

  // Appends the points after `from` up to and including `to`.
  const refine = (from: LinearPoint, fromStep: number, to: LinearPoint, toStep: number): void => {
    const span = toStep - fromStep
    const deviates = CHECKED_FRACTIONS.some((fraction) => {
      const step = fromStep + Math.round(span * fraction)
      if (step <= fromStep || step >= toStep) return false
      const probe = pointAt(step)
      const chord = from.value + ((to.value - from.value) * (step - fromStep)) / span
      return Math.abs(probe.value - chord) > tolerance
    })

    const canSplit = span > 1
    if (!deviates || !canSplit) {
      points.push(to)
      return
    }
    const middleStep = fromStep + Math.floor(span / 2)
    const middle = pointAt(middleStep)
    refine(from, fromStep, middle, middleStep)
    refine(middle, middleStep, to, toStep)
  }

  refine(points[0]!, 0, pointAt(POSITION_STEPS), POSITION_STEPS)
  return points
}

// Position-less first and last entries, as CSS allows: `linear(0, 0.1 5%, ..., 1)`.
export function toCssLinear(points: readonly LinearPoint[]): string {
  const last = points.length - 1
  const entries = points.map((point, index) => {
    const isEnd = index === 0 || index === last
    if (isEnd) return String(point.value)
    return `${point.value} ${roundTo(point.progress * 100, PERCENT_DECIMALS)}%`
  })
  return `linear(${entries.join(', ')})`
}

// What a browser draws for `points`: straight lines between them.
export function evaluateLinear(points: readonly LinearPoint[], progress: number): number {
  const index = points.findIndex((point) => point.progress >= progress)
  if (index <= 0) return points[0]!.value
  const before = points[index - 1]!
  const after = points[index]!
  const along = (progress - before.progress) / (after.progress - before.progress)
  return before.value + (after.value - before.value) * along
}
