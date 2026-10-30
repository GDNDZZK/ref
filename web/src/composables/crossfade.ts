import { nextTick } from 'vue'

// Runs `change` under a snapshot of the old look and fades the snapshot out (base.css), so every
// color crossfades at once without per-element transitions. Used for the theme and for switching
// the shown session. Without view transitions the change applies at once.
export async function crossfade(change: () => void, speed: 'slow' | 'standard' = 'slow'): Promise<void> {
  if (!document.startViewTransition) return change()
  document.documentElement.dataset['fade'] = speed
  const transition = document.startViewTransition(async () => {
    change()
    await nextTick() // the new DOM must be in place when the snapshot is taken
  })
  void transition.finished.catch(() => undefined).then(() => delete document.documentElement.dataset['fade'])
  await transition.updateCallbackDone
}
