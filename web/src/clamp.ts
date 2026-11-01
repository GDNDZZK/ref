// `value` held within [min, max]. An empty range (min > max) gives max.
export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}
