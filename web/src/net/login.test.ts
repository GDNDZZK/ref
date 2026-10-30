import { afterEach, describe, expect, it, vi } from 'vitest'
import { loginRequired } from './login.ts'

afterEach(() => vi.unstubAllGlobals())

describe('loginRequired', () => {
  it('is true when ref-web answers 401', async () => {
    vi.stubGlobal('fetch', () => Promise.resolve(new Response('', { status: 401 })))
    expect(await loginRequired()).toBe(true)
  })

  it('is false when logged in', async () => {
    vi.stubGlobal('fetch', () => Promise.resolve(new Response('{}', { status: 200 })))
    expect(await loginRequired()).toBe(false)
  })

  it('is false when ref-web cannot be reached', async () => {
    vi.stubGlobal('fetch', () => Promise.reject(new TypeError('network')))
    expect(await loginRequired()).toBe(false)
  })
})
