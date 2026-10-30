import { easings } from './easing.ts'
import type { EasingName } from './easing.ts'
import { subscribeToFrames } from './frameLoop.ts'

// Animates a number from `from` to `to` with a Qt easing curve (NumberAnimation in QML).
// `cancel()` stops where it is.
export function tween(options: {
  from: number
  to: number
  durationMs: number
  easing: EasingName
  update(value: number): void
  done?(): void
}): () => void {
  const { from, to, durationMs, update, done } = options
  const ease = easings[options.easing]
  let startedAt: number | undefined

  const stop = subscribeToFrames((_frameSeconds, now) => {
    startedAt ??= now
    const progress = Math.min(1, (now - startedAt) / durationMs)
    update(from + (to - from) * ease(progress))
    if (progress < 1) return
    stop()
    done?.()
  })
  return stop
}
