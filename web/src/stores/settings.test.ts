import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import { STORAGE_KEY } from '../settings/model.ts'
import { useNoticesStore } from './notices.ts'
import { useSettingsStore } from './settings.ts'

beforeEach(() => {
  createAppI18n(['en'])
  localStorage.clear()
  setActivePinia(createPinia())
})

describe('settings store', () => {
  it('saves a change to localStorage', () => {
    const store = useSettingsStore()
    store.update((draft) => (draft.sidebarOpen = false))
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({ sidebarOpen: false })
  })

  it('loads what an earlier visit saved', () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ theme: 'dark' }))
    const store = useSettingsStore()
    expect(store.settings.theme).toBe('dark')
    expect(store.isDark).toBe(true)
  })

  it('uses the defaults, tells the user once and leaves the stored text alone when it is not JSON', () => {
    localStorage.setItem(STORAGE_KEY, '{oops')
    const store = useSettingsStore()
    expect(store.settings.theme).toBe('system')
    expect(useNoticesStore().notices.map((notice) => notice.level)).toEqual(['warn'])
    expect(localStorage.getItem(STORAGE_KEY)).toBe('{oops')
  })

  it('follows a change made in another tab', () => {
    const store = useSettingsStore()
    const saved = JSON.stringify({ language: 'zh-CN', theme: 'light' })
    window.dispatchEvent(new StorageEvent('storage', { key: STORAGE_KEY, newValue: saved }))
    expect(store.locale).toBe('zh-CN')
    expect(store.isDark).toBe(false)
  })

  it('goes back to the defaults when another tab clears the storage', () => {
    const store = useSettingsStore()
    store.update((draft) => (draft.theme = 'dark'))
    window.dispatchEvent(new StorageEvent('storage', { key: null, newValue: null }))
    expect(store.settings.theme).toBe('system')
  })

  it('asks for no notifications when they are switched off', () => {
    const store = useSettingsStore()
    expect(store.notifyKinds).toEqual(['permission', 'question', 'turn_done', 'turn_error'])
    store.update((draft) => (draft.notifications.enabled = false))
    expect(store.notifyKinds).toEqual([])
  })
})
