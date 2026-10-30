import { describe, expect, it } from 'vitest'
import { resolveStartup, StartupError } from './startup.ts'

const URL = 'ws://example/ws'

function environment(overrides: Partial<Parameters<typeof resolveStartup>[0]> = {}) {
  return {
    search: '',
    directUrl: undefined,
    directCwd: undefined,
    fetchInfo: () => Promise.resolve({ cwd: '/from/info' }),
    ...overrides,
  }
}

describe('resolveStartup', () => {
  it('prefers ?dir= over /api/info', async () => {
    const startup = await resolveStartup(environment({ search: '?dir=%2Fwork%2Fa' }), URL)
    expect(startup).toEqual({ url: URL, cwd: '/work/a', sessionId: undefined, split: false })
  })

  it('falls back to /api/info.cwd', async () => {
    const startup = await resolveStartup(environment(), URL)
    expect(startup.cwd).toBe('/from/info')
  })

  it('reads ?view=split', async () => {
    expect((await resolveStartup(environment({ search: '?view=split' }), URL)).split).toBe(true)
    expect((await resolveStartup(environment({ search: '?view=other' }), URL)).split).toBe(false)
  })

  it('reads ?session=', async () => {
    const startup = await resolveStartup(environment({ search: '?session=ses_1' }), URL)
    expect(startup.sessionId).toBe('ses_1')
  })

  it('connecting directly takes ?cwd= or VITE_REF_CWD and never /api/info', async () => {
    const direct = {
      directUrl: 'ws://127.0.0.1:1/',
      fetchInfo: () => Promise.reject(new Error('no ref-web')),
    }
    expect((await resolveStartup(environment({ ...direct, search: '?cwd=/x' }), URL)).cwd).toBe('/x')
    expect((await resolveStartup(environment({ ...direct, directCwd: '/y' }), URL)).cwd).toBe('/y')
  })

  it('connecting directly without a directory is an error, not a default', async () => {
    const failure = resolveStartup(environment({ directUrl: 'ws://127.0.0.1:1/' }), URL)
    await expect(failure).rejects.toBeInstanceOf(StartupError)
    await expect(failure).rejects.toMatchObject({ messageKey: 'connection.noDirectory' })
  })
})
