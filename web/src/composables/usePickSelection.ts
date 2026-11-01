// The highlighted row of a list that is searched and moved with the arrow keys.

import { ref, watch } from 'vue'
import type { Ref } from 'vue'
import { Key } from '../keys.ts'

export interface PickItem {
  key: string
  group?: string // section label above the first row of the group
  disabled?: boolean
}

// ArrowDown / ArrowUp move the highlight; Enter picks it when `onPick` is given. Returns whether
// the key was one of those (and so was kept from the text field).
export function handleListKey(
  event: KeyboardEvent,
  move: (delta: 1 | -1) => void,
  onPick?: () => void,
): boolean {
  if (event.key === Key.ArrowDown || event.key === Key.ArrowUp) {
    move(event.key === Key.ArrowDown ? 1 : -1)
    return consume(event)
  }
  if (event.key === Key.Enter && onPick) {
    onPick()
    return consume(event)
  }
  return false
}

function consume(event: KeyboardEvent): true {
  event.preventDefault()
  return true
}

export function usePickSelection(items: Ref<readonly PickItem[]>, preferredKey?: string) {
  const selected = ref<string | undefined>(preferredKey)

  const enabled = () => items.value.filter((item) => !item.disabled)

  // The list changed (typing): keep the highlight if its row is still there.
  watch(
    items,
    () => {
      if (enabled().some((item) => item.key === selected.value)) return
      selected.value = enabled()[0]?.key
    },
    { immediate: true },
  )

  // Wraps at both ends.
  function move(delta: 1 | -1): void {
    const rows = enabled()
    if (rows.length === 0) return
    const at = rows.findIndex((item) => item.key === selected.value)
    selected.value = rows[(at + delta + rows.length) % rows.length]?.key
  }

  return { selected, move }
}
