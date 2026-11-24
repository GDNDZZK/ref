import { describe, expect, it } from 'vitest'
import { resolveStartup, StartupError } from './startup.ts'

const URL = 'ws://example/ws'

function environment(overrides: Partial<Parameters<typeof resolveStartup>[0]> = {}) {
  return { search: '', directUrl: undefined, directCwd: undefined, ...overrides }
}

describe('resolveStartup', () => {
  it('takes ?dir=', () => {
    const startup = resolveStartup(environment({ search: '?dir=%2Fwork%2Fa' }), URL)
    expect(startup).toEqual({ url: URL, cwd: '/work/a', sessionId: undefined, split: false })
  })

  it('leaves the directory to ref-web (sealed frame 0) without ?dir=', () => {
    expect(resolveStartup(environment(), URL).cwd).toBeUndefined()
  })

  it('reads ?view=split', () => {
    expect(resolveStartup(environment({ search: '?view=split' }), URL).split).toBe(true)
    expect(resolveStartup(environment({ search: '?view=other' }), URL).split).toBe(false)
  })

  it('reads ?session=', () => {
    expect(resolveStartup(environment({ search: '?session=ses_1' }), URL).sessionId).toBe('ses_1')
  })

  it('connecting directly takes ?cwd= or VITE_REF_CWD', () => {
    const direct = { directUrl: 'ws://127.0.0.1:1/' }
    expect(resolveStartup(environment({ ...direct, search: '?cwd=/x' }), URL).cwd).toBe('/x')
    expect(resolveStartup(environment({ ...direct, directCwd: '/y' }), URL).cwd).toBe('/y')
  })

  it('connecting directly without a directory is an error, not a default', () => {
    const attempt = () => resolveStartup(environment({ directUrl: 'ws://127.0.0.1:1/' }), URL)
    expect(attempt).toThrow(StartupError)
    expect(attempt).toThrow(expect.objectContaining({ messageKey: 'connection.noDirectory' }))
  })

  it('supports ?server= and ?token= as Direct WS overrides and rejects invalid ?server=', () => {
    expect(
      resolveStartup(
        environment({ search: '?server=wss%3A%2F%2Fhost%3A8080%2Fws&token=abc&dir=%2Fproj' }),
        URL,
      ),
    ).toEqual({
      url: 'wss://host:8080/ws',
      cwd: '/proj',
      sessionId: undefined,
      split: false,
      direct: true,
      token: 'abc',
    })
    expect(() => resolveStartup(environment({ search: '?server=http://bad' }), URL)).toThrow(StartupError)
  })
})
