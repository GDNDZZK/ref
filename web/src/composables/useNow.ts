// The current time in ms, updated every second while someone uses it (running time counters).

import { onScopeDispose, ref } from 'vue'
import type { Ref } from 'vue'

const TICK_MS = 1000

const now = ref(Date.now())
let users = 0
let timer: ReturnType<typeof setInterval> | undefined

export function useNow(): Readonly<Ref<number>> {
  if (users++ === 0) {
    now.value = Date.now()
    timer = setInterval(() => (now.value = Date.now()), TICK_MS)
  }
  onScopeDispose(() => {
    if (--users === 0) clearInterval(timer)
  })
  return now
}
