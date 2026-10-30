import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import SendButton from './SendButton.vue'

function mountButton(props: { busy: boolean; canSend: boolean }) {
  return mount(SendButton, { props, global: { plugins: [createAppI18n(['en'])] } })
}

describe('SendButton', () => {
  it('is inert without text and while idle', async () => {
    const wrapper = mountButton({ busy: false, canSend: false })
    await wrapper.trigger('click')
    expect(wrapper.emitted('click')).toBeUndefined()
    expect(wrapper.attributes('aria-disabled')).toBe('true')
  })

  it('sends when there is text', async () => {
    const wrapper = mountButton({ busy: false, canSend: true })
    await wrapper.trigger('click')
    expect(wrapper.emitted('click')).toHaveLength(1)
    expect(wrapper.attributes('aria-label')).toBe('Send')
  })

  it('turns into stop while busy, even without text', async () => {
    const wrapper = mountButton({ busy: true, canSend: false })
    expect(wrapper.attributes('aria-label')).toBe('Stop')
    expect(wrapper.classes()).toContain('busy')
    await wrapper.trigger('click')
    expect(wrapper.emitted('click')).toHaveLength(1)
  })
})
