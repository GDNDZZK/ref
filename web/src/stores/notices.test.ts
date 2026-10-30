import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useNoticesStore } from './notices.ts'

beforeEach(() => {
  vi.useFakeTimers()
  setActivePinia(createPinia())
})

afterEach(() => vi.useRealTimers())

describe('notices store', () => {
  it('keeps a toast for a time that depends on its level', () => {
    const store = useNoticesStore()
    store.push('info', 'saved')
    store.push('error', 'failed')
    vi.advanceTimersByTime(3500)
    expect(store.notices.map((notice) => notice.text)).toEqual(['failed'])
    vi.advanceTimersByTime(4500)
    expect(store.notices).toEqual([])
  })

  it('restarts the toast that is already there instead of stacking a second one', () => {
    const store = useNoticesStore()
    store.push('info', 'saved')
    vi.advanceTimersByTime(3000)
    store.push('info', 'saved')
    vi.advanceTimersByTime(3000)
    expect(store.notices).toHaveLength(1)
    expect(store.notices[0]?.repeats).toBe(1)
    vi.advanceTimersByTime(500)
    expect(store.notices).toEqual([])
  })

  it('shows the same text of another level as a separate toast', () => {
    const store = useNoticesStore()
    store.push('info', 'x')
    store.push('error', 'x')
    expect(store.notices).toHaveLength(2)
  })

  it('shows at most four and drops the oldest first', () => {
    const store = useNoticesStore()
    for (const text of ['a', 'b', 'c', 'd', 'e']) store.push('info', text)
    expect(store.notices.map((notice) => notice.text)).toEqual(['b', 'c', 'd', 'e'])
  })

  it('stops the clock while the pointer is over a toast', () => {
    const store = useNoticesStore()
    store.push('info', 'saved')
    const id = store.notices[0]!.id
    vi.advanceTimersByTime(3000)
    store.pause(id)
    vi.advanceTimersByTime(60_000)
    expect(store.notices).toHaveLength(1)
    store.resume(id)
    vi.advanceTimersByTime(500)
    expect(store.notices).toEqual([])
  })
})
