// One requestAnimationFrame loop shared by every per-frame animation (FrameAnimation in QML).
// It runs only while someone is subscribed.

export type FrameCallback = (frameSeconds: number, now: number) => void

const MS_PER_SECOND = 1000

const subscribers = new Set<FrameCallback>()
let handle: number | undefined
let previousTime: number | undefined

function tick(now: number): void {
  const frameSeconds = previousTime === undefined ? 0 : (now - previousTime) / MS_PER_SECOND
  previousTime = now
  handle = undefined
  // Callbacks may unsubscribe themselves or others while we iterate.
  for (const callback of [...subscribers]) {
    if (subscribers.has(callback)) callback(frameSeconds, now)
  }
  schedule()
}

function schedule(): void {
  if (handle !== undefined || subscribers.size === 0) return
  handle = requestAnimationFrame(tick)
}

// Returns the function that ends the subscription.
export function subscribeToFrames(callback: FrameCallback): () => void {
  subscribers.add(callback)
  schedule()
  return () => {
    subscribers.delete(callback)
    if (subscribers.size === 0 && handle !== undefined) {
      cancelAnimationFrame(handle)
      handle = undefined
      previousTime = undefined
    }
  }
}
