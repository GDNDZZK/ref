import { flushPromises, mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import type { NotificationPermissionState } from '../notify/notifier.ts'
import { STORAGE_KEY } from '../settings/model.ts'
import { useSettingsStore } from '../stores/settings.ts'
import NotificationPrompt from './NotificationPrompt.vue'
import NotificationSettings from './NotificationSettings.vue'

function installNotification(permission: NotificationPermissionState) {
  const request = vi.fn(async () => 'granted' as NotificationPermissionState)
  class FakeNotification {
    static permission = permission
    static requestPermission = request
    onclick = null
    close() {}
  }
  vi.stubGlobal('Notification', FakeNotification)
  return request
}

function mountWith<T extends typeof NotificationSettings | typeof NotificationPrompt>(component: T) {
  const pinia = createPinia()
  const wrapper = mount(component, { global: { plugins: [pinia, createAppI18n(['en'])] } })
  return { wrapper, settings: useSettingsStore(pinia) }
}

beforeEach(() => {
  localStorage.clear()
  Object.defineProperty(window, 'isSecureContext', { value: true, configurable: true })
})

afterEach(() => vi.unstubAllGlobals())

describe('NotificationSettings', () => {
  it('shows the master switch and the three kinds, which follow the master', async () => {
    installNotification('granted')
    const { wrapper, settings } = mountWith(NotificationSettings)
    const switches = wrapper.findAll('[role="switch"]')
    expect(switches).toHaveLength(4)
    expect(switches.every((item) => item.attributes('aria-checked') === 'true')).toBe(true)

    await switches[0]!.trigger('click')
    expect(settings.settings.notifications.enabled).toBe(false)
    expect(wrapper.findAll('.kind.dimmed')).toHaveLength(3)
    expect(wrapper.findAll('.kind button').every((item) => item.attributes('disabled') !== undefined)).toBe(
      true,
    )
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!).notifications.enabled).toBe(false)
  })

  it('switches one kind off without touching the others', async () => {
    installNotification('granted')
    const { wrapper, settings } = mountWith(NotificationSettings)
    await wrapper.findAll('[role="switch"]')[2]!.trigger('click')
    expect(settings.settings.notifications).toEqual({
      enabled: true,
      permission: true,
      turnDone: false,
      turnError: true,
    })
  })

  it('asks the browser for permission when the master switch goes on', async () => {
    const request = installNotification('default')
    const { wrapper, settings } = mountWith(NotificationSettings)
    settings.update((draft) => (draft.notifications.enabled = false))
    await flushPromises()
    expect(request).not.toHaveBeenCalled()
    await wrapper.findAll('[role="switch"]')[0]!.trigger('click')
    expect(request).toHaveBeenCalledOnce()
  })

  it('offers an Allow button while the permission is undecided', async () => {
    const request = installNotification('default')
    const { wrapper } = mountWith(NotificationSettings)
    await wrapper.get('.ask button').trigger('click')
    expect(request).toHaveBeenCalledOnce()
  })

  it.each([
    ['denied', 'Blocked in the browser'],
    ['granted', ''],
  ] as const)('explains a %s permission', (permission, note) => {
    installNotification(permission)
    const { wrapper } = mountWith(NotificationSettings)
    expect(wrapper.find('.note').exists()).toBe(note !== '')
    if (note) expect(wrapper.get('.note').text()).toContain(note)
  })

  it('says so when the page is not a secure context', () => {
    installNotification('granted')
    Object.defineProperty(window, 'isSecureContext', { value: false, configurable: true })
    const { wrapper } = mountWith(NotificationSettings)
    expect(wrapper.get('.note').text()).toContain('https or localhost')
  })

  it('says so when the browser has no Notification API', () => {
    vi.stubGlobal('Notification', undefined)
    const { wrapper } = mountWith(NotificationSettings)
    expect(wrapper.get('.note').text()).toContain('cannot show notifications')
  })
})

describe('NotificationPrompt', () => {
  it('asks once and stays away after "Not now"', async () => {
    const request = installNotification('default')
    const first = mountWith(NotificationPrompt)
    await first.wrapper.get('button.allow').trigger('click')
    expect(request).toHaveBeenCalledOnce()

    const second = mountWith(NotificationPrompt)
    const buttons = second.wrapper.findAll('button')
    await buttons[1]!.trigger('click')
    expect(second.wrapper.find('.prompt').exists()).toBe(false)
    expect(mountWith(NotificationPrompt).wrapper.find('.prompt').exists()).toBe(false)
  })

  it('stays hidden when notifications are off or already decided', () => {
    installNotification('granted')
    expect(mountWith(NotificationPrompt).wrapper.find('.prompt').exists()).toBe(false)
    installNotification('default')
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ notifications: { enabled: false } }))
    expect(mountWith(NotificationPrompt).wrapper.find('.prompt').exists()).toBe(false)
  })
})
