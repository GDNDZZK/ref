import { describe, expect, it } from 'vitest'
import { defaultSettings, notifyKinds, parseSettings } from './model.ts'

describe('parseSettings', () => {
  it('gives the defaults when nothing is stored', () => {
    expect(parseSettings(null)).toEqual({ settings: defaultSettings(), corrupt: false })
  })

  it('gives the defaults and flags text that is not JSON', () => {
    expect(parseSettings('{oops')).toEqual({ settings: defaultSettings(), corrupt: true })
  })

  it('gives the defaults, without a flag, for JSON that is not an object', () => {
    expect(parseSettings('42')).toEqual({ settings: defaultSettings(), corrupt: false })
  })

  it('reads what was stored', () => {
    const stored = {
      theme: 'dark',
      language: 'zh-CN',
      notifications: { enabled: false, permission: true, turnDone: false, turnError: true },
      sidebarOpen: false,
    }
    expect(parseSettings(JSON.stringify(stored))).toEqual({ settings: stored, corrupt: false })
  })

  it('replaces only the fields that are invalid or missing', () => {
    const { settings } = parseSettings(JSON.stringify({ theme: 'sepia', language: 'en', sidebarOpen: 'no' }))
    expect(settings).toEqual({ ...defaultSettings(), language: 'en' })
  })
})

describe('notifyKinds', () => {
  it('lists the enabled kinds', () => {
    const notifications = { enabled: true, permission: true, turnDone: false, turnError: true }
    expect(notifyKinds(notifications)).toEqual(['permission', 'question', 'turn_error'])
  })

  it('is empty when the master switch is off', () => {
    expect(notifyKinds({ ...defaultSettings().notifications, enabled: false })).toEqual([])
  })
})
