// The submit button of a dialog that asks the backend for something: one request at a time, a
// failure shows as the dialog's error text.

import { ref } from 'vue'
import { describeError } from '../errorMessage.ts'

export function useDialogSubmit(action: () => Promise<void>, isReady: () => boolean) {
  const busy = ref(false)
  const error = ref('')

  async function submit(): Promise<void> {
    if (!isReady() || busy.value) return
    busy.value = true
    try {
      await action()
    } catch (failure) {
      error.value = describeError(failure)
    } finally {
      busy.value = false
    }
  }

  return { busy, error, submit }
}
