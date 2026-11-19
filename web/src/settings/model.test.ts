import { describe, expect, it } from 'vitest'
import { metrics } from '../theme/generated/theme.ts'
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
      sidebarWidth: 340,
      sidebarTodoExpanded: false,
      sidebarRunningExpanded: false,
      sidebarHistoryExpanded: false,
      sidebarRunningOrder: ['s2', 's1'],
    }
    expect(parseSettings(JSON.stringify(stored))).toEqual({ settings: stored, corrupt: false })
  })

  it('replaces only the fields that are invalid or missing', () => {
    const { settings } = parseSettings(
      JSON.stringify({
        theme: 'sepia',
        language: 'en',
        sidebarOpen: 'no',
        sidebarTodoExpanded: 'yes',
        sidebarRunningExpanded: 0,
      }),
    )
    expect(settings).toEqual({ ...defaultSettings(), language: 'en' })
  })

  it('clamps sidebarWidth to bounds and falls back on bad or non-finite values', () => {
    expect(parseSettings(JSON.stringify({ sidebarWidth: 100 })).settings.sidebarWidth).toBe(
      metrics.sidebarMinWidth,
    )
    expect(parseSettings(JSON.stringify({ sidebarWidth: 900 })).settings.sidebarWidth).toBe(
      metrics.sidebarMaxWidth,
    )
    expect(parseSettings(JSON.stringify({ sidebarWidth: 320.6 })).settings.sidebarWidth).toBe(321)
    expect(parseSettings(JSON.stringify({ sidebarWidth: 'wide' })).settings.sidebarWidth).toBe(
      metrics.sidebarWidth,
    )
    expect(parseSettings(JSON.stringify({ sidebarWidth: null })).settings.sidebarWidth).toBe(
      metrics.sidebarWidth,
    )
  })

  it('deduplicates sidebarRunningOrder and drops invalid entries', () => {
    expect(parseSettings(JSON.stringify({ sidebarRunningOrder: 's1' })).settings.sidebarRunningOrder).toEqual(
      [],
    )
    expect(
      parseSettings(JSON.stringify({ sidebarRunningOrder: ['s2', '', 42, null, 's1', 's2', 's3', false] }))
        .settings.sidebarRunningOrder,
    ).toEqual(['s2', 's1', 's3'])
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
