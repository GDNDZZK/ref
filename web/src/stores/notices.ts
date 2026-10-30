// Short messages for the user: protocol toasts and errors from failed requests.

import { defineStore } from 'pinia'
import { ref } from 'vue'

export type NoticeLevel = 'info' | 'warn' | 'error'

export interface Notice {
  id: number
  level: NoticeLevel
  text: string
}

const SHOWN_FOR_MS = 6000

export const useNoticesStore = defineStore('notices', () => {
  const notices = ref<Notice[]>([])
  let nextId = 1

  function dismiss(id: number): void {
    notices.value = notices.value.filter((notice) => notice.id !== id)
  }

  function push(level: NoticeLevel, text: string): void {
    const id = nextId++
    notices.value.push({ id, level, text })
    setTimeout(() => dismiss(id), SHOWN_FOR_MS)
  }

  function pushError(error: unknown): void {
    push('error', error instanceof Error ? error.message : String(error))
  }

  return { notices, push, pushError, dismiss }
})
