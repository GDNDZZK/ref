// A copy action that shows "copied" for a moment afterwards. A failed copy goes to the notices.

import { onBeforeUnmount, ref } from 'vue'
import { useNoticesStore } from '../stores/notices.ts'

export function useCopyFeedback(write: () => Promise<void>, shownMs: number) {
  const notices = useNoticesStore()
  const copied = ref(false)
  let resetTimer: ReturnType<typeof setTimeout> | undefined

  async function copy(): Promise<void> {
    try {
      await write()
    } catch (error) {
      notices.pushError(error)
      return
    }
    copied.value = true
    clearTimeout(resetTimer)
    resetTimer = setTimeout(() => (copied.value = false), shownMs)
  }

  onBeforeUnmount(() => clearTimeout(resetTimer))
  return { copied, copy }
}
