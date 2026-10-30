// Time left until `runAt`, refreshed every frame while `active`. `total` is the time that was
// left when the countdown was first seen, so a card that appears mid-delay starts partly drawn.

import { ref, watch } from 'vue'
import type { Ref } from 'vue'
import { useFrame } from '../motion/useFrame.ts'

export function useCountdown(runAt: Readonly<Ref<number | null>>, active: Readonly<Ref<boolean>>) {
  const remaining = ref(0)
  const total = ref(0)
  const ticking = ref(false)

  const left = () => Math.max(0, (runAt.value ?? 0) - Date.now())

  watch(
    active,
    (isActive) => {
      if (isActive) {
        remaining.value = left()
        total.value = remaining.value
      }
      ticking.value = isActive
    },
    { immediate: true },
  )

  useFrame(ticking, () => {
    remaining.value = left()
    if (remaining.value === 0) ticking.value = false
  })

  return { remaining, total }
}
