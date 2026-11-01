import { describe, expect, it, vi } from 'vitest'
import { useDialogSubmit } from './useDialogSubmit.ts'

describe('useDialogSubmit', () => {
  it('runs the action once at a time', async () => {
    let finish!: () => void
    const action = vi.fn(() => new Promise<void>((resolve) => (finish = resolve)))
    const { busy, submit } = useDialogSubmit(action, () => true)
    const first = submit()
    await submit()
    expect(busy.value).toBe(true)
    expect(action).toHaveBeenCalledOnce()
    finish()
    await first
    expect(busy.value).toBe(false)
  })

  it('does nothing while not ready', async () => {
    const action = vi.fn().mockResolvedValue(undefined)
    await useDialogSubmit(action, () => false).submit()
    expect(action).not.toHaveBeenCalled()
  })

  it('shows a failure as the error text and is ready again', async () => {
    const { busy, error, submit } = useDialogSubmit(
      () => Promise.reject(new Error('no space')),
      () => true,
    )
    await submit()
    expect(error.value).toBe('no space')
    expect(busy.value).toBe(false)
  })
})
