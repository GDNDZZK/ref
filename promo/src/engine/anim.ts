// Animation as a pure function of time. Times are in seconds.
import { easings, type EasingName } from '../../../web/src/motion/easing.ts'

export type { EasingName }

export function lerp(from: number, to: number, progress: number): number {
  return from + (to - from) * progress
}

// Eased progress of an animation that starts at `start` and lasts `duration`.
// 0 before it starts, 1 after it ends (the easing curves clamp their input).
export function span(time: number, start: number, duration: number, easing: EasingName = 'linear'): number {
  return easings[easing]((time - start) / duration)
}

// 1 between `from` and `to`, with fades of `fade` seconds around that interval.
export function fadeWindow(time: number, from: number, to: number, fade: number): number {
  const fadeIn = span(time, from, fade, 'inOutSine')
  const fadeOut = 1 - span(time, to - fade, fade, 'inOutSine')
  return Math.min(fadeIn, fadeOut)
}

export interface Key {
  at: number
  value: number
  // Curve of the segment that ends at this key.
  ease?: EasingName
}

const DEFAULT_SEGMENT_EASING: EasingName = 'inOutSine'

// Piecewise animation through `keys` (sorted by `at`). Holds the first value before the
// first key and the last value after the last key.
export function track(keys: readonly Key[]): (time: number) => number {
  const first = keys[0]
  const last = keys[keys.length - 1]
  if (!first || !last) throw new Error('track needs at least one key')

  return (time) => {
    if (time <= first.at) return first.value
    if (time >= last.at) return last.value

    const nextIndex = keys.findIndex((key) => key.at > time)
    const from = keys[nextIndex - 1]!
    const to = keys[nextIndex]!
    const progress = span(time, from.at, to.at - from.at, to.ease ?? DEFAULT_SEGMENT_EASING)
    return lerp(from.value, to.value, progress)
  }
}
