// The current time in ms, refreshed every second while a component uses it. For counters
// (running 12s, retrying in 4s) that must tick without any event arriving.

import { computed, onScopeDispose, ref } from 'vue'
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
    if (--users > 0) return
    clearInterval(timer)
    timer = undefined
  })
  return computed(() => now.value)
}
