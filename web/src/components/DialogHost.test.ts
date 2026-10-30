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
    expect(dialogs.current).toBeDefined()
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
})
