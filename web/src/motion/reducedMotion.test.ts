import { afterEach, describe, expect, it, vi } from 'vitest'
import { crossfade } from '../composables/crossfade.ts'
import { StreamPacer } from '../model/streamPacer.ts'
import { chase, scrollStep } from './chase.ts'
import { prefersReducedMotion } from './reducedMotion.ts'
import { tween } from './tween.ts'

describe('prefersReducedMotion', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('snaps tween, chase, scrollStep, StreamPacer, and crossfade immediately when active', async () => {
    vi.stubGlobal('matchMedia', (query: string) => ({
      matches: query.includes('prefers-reduced-motion'),
      addEventListener: () => {},
    }))
    expect(prefersReducedMotion()).toBe(true)

    const values: number[] = []
    let finished = false
    tween({
      from: 100,
      to: 0,
      durationMs: 480,
      easing: 'outExpo',
      update: (v) => values.push(v),
      done: () => {
        finished = true
      },
    })
    expect(values).toEqual([0])
    expect(finished).toBe(true)

    expect(chase(10, 200, 0.016)).toBe(200)
    expect(scrollStep(10, 200, 0.016)).toBe(200)

    const pacer = new StreamPacer()
    pacer.push({ stream: 'p1', text: 'hello world' }, 100)
    expect(pacer.take(101)).toEqual([{ stream: 'p1', text: 'hello world' }])

    const startVt = vi.fn()
    Object.defineProperty(document, 'startViewTransition', {
      configurable: true,
      value: startVt,
    })
    let applied = false
    await crossfade(() => {
      applied = true
    })
    expect(applied).toBe(true)
    expect(startVt).not.toHaveBeenCalled()
  })
})
