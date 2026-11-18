import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import { useDialogsStore } from '../stores/dialogs.ts'
import DialogHost from './DialogHost.vue'

function mountHost() {
  const pinia = createPinia()
  setActivePinia(pinia)
  const wrapper = mount(DialogHost, {
    attachTo: document.body,
    global: { plugins: [pinia, createAppI18n(['en'])], stubs: { transition: false } },
  })
  return { wrapper, dialogs: useDialogsStore() }
}

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
  document.body.innerHTML = ''
})

describe('DialogHost', () => {
  it('shows nothing until a dialog is requested', () => {
    const { wrapper } = mountHost()
    expect(wrapper.find('.scrim').exists()).toBe(false)
  })

  it('shows the requested dialog and closes it with Esc', async () => {
    const { wrapper, dialogs } = mountHost()
    dialogs.open({ kind: 'help' })
    await nextTick()
    expect(wrapper.find('[role="dialog"]').attributes('aria-label')).toBe('Help')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(dialogs.current).toBeUndefined()
  })

  it('closes on a click on the scrim but not on the panel', async () => {
    const { wrapper, dialogs } = mountHost()
    dialogs.open({ kind: 'help' })
    await nextTick()
    await wrapper.find('[role="dialog"]').trigger('mousedown')
    expect(dialogs.current).toEqual({ kind: 'help' })
    await wrapper.find('.scrim').trigger('mousedown')
    expect(dialogs.current).toBeUndefined()
  })

  it('calls back after closing, to give the focus back to the composer', async () => {
    const { dialogs } = mountHost()
    const closed = vi.fn()
    dialogs.setOnClosed(closed)
    dialogs.open({ kind: 'help' })
    await nextTick()
    dialogs.close()
    expect(closed).toHaveBeenCalledOnce()
  })

  it('prevents wheel bleed-through on the scrim and non-scrollable dialog elements', async () => {
    const { wrapper, dialogs } = mountHost()
    dialogs.open({ kind: 'help' })
    await nextTick()

    const scrim = wrapper.get('.scrim').element as HTMLElement
    const scrimWheel = new WheelEvent('wheel', { deltaY: 80, bubbles: true, cancelable: true })
    scrim.dispatchEvent(scrimWheel)
    expect(scrimWheel.defaultPrevented).toBe(true)

    const dialog = wrapper.get('[role="dialog"]').element as HTMLElement
    const dialogWheel = new WheelEvent('wheel', { deltaY: 80, bubbles: true, cancelable: true })
    dialog.dispatchEvent(dialogWheel)
    expect(dialogWheel.defaultPrevented).toBe(true)
  })

  it('allows wheel inside a scrollable dialog container until it reaches top or bottom', async () => {
    const { wrapper, dialogs } = mountHost()
    dialogs.open({ kind: 'help' })
    await nextTick()

    const content = wrapper.get('[role="dialog"] .content').element as HTMLElement
    content.style.overflowY = 'auto'
    let scrollTop = 50
    Object.defineProperty(content, 'clientHeight', { configurable: true, get: () => 100 })
    Object.defineProperty(content, 'scrollHeight', { configurable: true, get: () => 300 })
    Object.defineProperty(content, 'scrollTop', {
      configurable: true,
      get: () => scrollTop,
      set: (value: number) => {
        scrollTop = value
      },
    })

    const child = content.firstElementChild ?? content
    const midDown = new WheelEvent('wheel', { deltaY: 40, bubbles: true, cancelable: true })
    child.dispatchEvent(midDown)
    expect(midDown.defaultPrevented).toBe(false)

    const midUp = new WheelEvent('wheel', { deltaY: -40, bubbles: true, cancelable: true })
    child.dispatchEvent(midUp)
    expect(midUp.defaultPrevented).toBe(false)

    scrollTop = 0
    const topUp = new WheelEvent('wheel', { deltaY: -40, bubbles: true, cancelable: true })
    child.dispatchEvent(topUp)
    expect(topUp.defaultPrevented).toBe(true)

    const topDown = new WheelEvent('wheel', { deltaY: 40, bubbles: true, cancelable: true })
    child.dispatchEvent(topDown)
    expect(topDown.defaultPrevented).toBe(false)

    scrollTop = 200
    const bottomDown = new WheelEvent('wheel', { deltaY: 40, bubbles: true, cancelable: true })
    child.dispatchEvent(bottomDown)
    expect(bottomDown.defaultPrevented).toBe(true)

    const bottomUp = new WheelEvent('wheel', { deltaY: -40, bubbles: true, cancelable: true })
    child.dispatchEvent(bottomUp)
    expect(bottomUp.defaultPrevented).toBe(false)
  })

  it('switches from top-aligned CommandPalette to centered dialogs without dropping the scrim', async () => {
    const { wrapper, dialogs } = mountHost()
    dialogs.open({ kind: 'palette' })
    await nextTick()
    expect(wrapper.find('.panel-slot.top').exists()).toBe(true)

    dialogs.open({ kind: 'models', sessionId: 's1' })
    await nextTick()
    expect(wrapper.find('.scrim').exists()).toBe(true)
    expect(wrapper.text()).toContain('No models configured')
    expect(wrapper.text()).toContain('Open configuration…')
  })
})
