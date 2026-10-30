// Toasts: short messages for the user, from the protocol and from failed requests.

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { motion } from '../theme/generated/motion.ts'
import { metrics } from '../theme/generated/theme.ts'

export type NoticeLevel = 'info' | 'warn' | 'error'

export interface Notice {
  id: number
  level: NoticeLevel
  text: string
  repeats: number // grows when the same message arrives again while it shows
}

const SHOWN_FOR_MS: Record<NoticeLevel, number> = {
  info: motion.toastInfoMs,
  warn: motion.toastWarnMs,
  error: motion.toastErrorMs,
}
const MAX_SHOWN = metrics.toastMax

export function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

interface Timer {
  handle: ReturnType<typeof setTimeout>
  startedAt: number
  remaining: number
}

export const useNoticesStore = defineStore('notices', () => {
  const notices = ref<Notice[]>([])
  const timers = new Map<number, Timer>()
  let nextId = 1

  function dismiss(id: number): void {
    clearTimeout(timers.get(id)?.handle)
    timers.delete(id)
    notices.value = notices.value.filter((notice) => notice.id !== id)
  }

  function start(id: number, ms: number): void {
    const handle = setTimeout(() => dismiss(id), ms)
    timers.set(id, { handle, startedAt: Date.now(), remaining: ms })
  }

  // While the pointer is over a toast its time stands still.
  function pause(id: number): void {
    const timer = timers.get(id)
    if (!timer) return
    clearTimeout(timer.handle)
    timer.remaining -= Date.now() - timer.startedAt
  }

  function resume(id: number): void {
    const timer = timers.get(id)
    if (timer) start(id, timer.remaining)
  }

  // The same message again restarts the one on screen instead of stacking.
  function push(level: NoticeLevel, text: string): void {
    const same = notices.value.find((notice) => notice.level === level && notice.text === text)
    if (same) {
      clearTimeout(timers.get(same.id)?.handle)
      start(same.id, SHOWN_FOR_MS[level])
      same.repeats++
      return
    }
    const id = nextId++
    notices.value.push({ id, level, text, repeats: 0 })
    start(id, SHOWN_FOR_MS[level])
    while (notices.value.length > MAX_SHOWN) dismiss(notices.value[0]!.id)
  }

  function pushError(error: unknown): void {
    push('error', describeError(error))
  }

  return { notices, push, pushError, dismiss, pause, resume }
})
