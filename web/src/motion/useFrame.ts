import { onScopeDispose, watch } from 'vue'
import type { WatchSource } from 'vue'
import { subscribeToFrames } from './frameLoop.ts'
import type { FrameCallback } from './frameLoop.ts'

// Calls `callback` every frame while `running` is true.
export function useFrame(running: WatchSource<boolean>, callback: FrameCallback): void {
  let stop: (() => void) | undefined
  const end = () => {
    stop?.()
    stop = undefined
  }

  watch(
    running,
    (isRunning) => {
      if (isRunning && !stop) stop = subscribeToFrames(callback)
      if (!isRunning) end()
    },
    { immediate: true },
  )
  onScopeDispose(end)
}
