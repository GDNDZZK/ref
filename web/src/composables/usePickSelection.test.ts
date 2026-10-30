import { describe, expect, it } from 'vitest'
import { nextTick, ref } from 'vue'
import { usePickSelection } from './usePickSelection.ts'
import type { PickItem } from './usePickSelection.ts'

const items = (...keys: string[]): PickItem[] => keys.map((key) => ({ key }))

describe('usePickSelection', () => {
  it('starts on the preferred row, else the first', () => {
    expect(usePickSelection(ref(items('a', 'b')), 'b').selected.value).toBe('b')
    expect(usePickSelection(ref(items('a', 'b'))).selected.value).toBe('a')
  })

  it('moves with wrap-around and skips disabled rows', () => {
    const list = ref<PickItem[]>([{ key: 'a' }, { key: 'b', disabled: true }, { key: 'c' }])
    const { selected, move } = usePickSelection(list)
    move(1)
    expect(selected.value).toBe('c')
    move(1)
    expect(selected.value).toBe('a')
    move(-1)
    expect(selected.value).toBe('c')
  })

  it('keeps the row while the list changes and falls back when it is gone', async () => {
    const list = ref(items('a', 'b', 'c'))
    const { selected, move } = usePickSelection(list)
    move(1)
    list.value = items('b', 'c')
    await nextTick()
    expect(selected.value).toBe('b')
    list.value = items('c')
    await nextTick()
    expect(selected.value).toBe('c')
    list.value = []
    await nextTick()
    expect(selected.value).toBeUndefined()
  })
})
