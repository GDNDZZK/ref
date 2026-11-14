import { describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'
import { handleListKey, usePickSelection } from './usePickSelection.ts'
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

describe('handleListKey', () => {
  const press = (key: string) => new KeyboardEvent('keydown', { key, cancelable: true })

  it('moves with the arrows and keeps the key from the text field', () => {
    const move = vi.fn()
    const down = press('ArrowDown')
    expect(handleListKey(down, move)).toBe(true)
    expect(down.defaultPrevented).toBe(true)
    expect(handleListKey(press('ArrowUp'), move)).toBe(true)
    expect(move.mock.calls).toEqual([[1], [-1]])
  })

  it('picks with Enter only when it is asked to', () => {
    const onPick = vi.fn()
    expect(handleListKey(press('Enter'), vi.fn())).toBe(false)
    expect(handleListKey(press('Enter'), vi.fn(), onPick)).toBe(true)
    expect(onPick).toHaveBeenCalledOnce()
  })

  it('leaves other keys alone', () => {
    const event = press('a')
    expect(handleListKey(event, vi.fn(), vi.fn())).toBe(false)
    expect(event.defaultPrevented).toBe(false)
  })
})
