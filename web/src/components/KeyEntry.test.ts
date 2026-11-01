import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import { useConnectionStore } from '../stores/connection.ts'
import KeyEntry from './KeyEntry.vue'

function mountEntry(locale = 'en') {
  const pinia = createPinia()
  setActivePinia(pinia)
  const connection = useConnectionStore()
  let answer: (accepted: boolean) => void = () => {}
  const submitKey = vi
    .spyOn(connection, 'submitKey')
    .mockImplementation(() => new Promise<boolean>((resolve) => (answer = resolve)))
  const wrapper = mount(KeyEntry, {
    attachTo: document.body,
    global: { plugins: [pinia, createAppI18n([locale])] },
  })
  return { wrapper, submitKey, answer: (accepted: boolean) => answer(accepted) }
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('KeyEntry', () => {
  it('asks for the key in a password field, with the hint', () => {
    const { wrapper } = mountEntry()
    expect(wrapper.find('h1').text()).toBe('Enter the key')
    expect(wrapper.find('input').attributes('type')).toBe('password')
    expect(wrapper.text()).toContain('<state dir>/web/login-token')
    expect(wrapper.find('button').text()).toBe('Sign in')
    expect(wrapper.find('button').attributes('disabled')).toBeDefined()
  })

  it('shows the check while it runs and the refusal after, keeping the text', async () => {
    const { wrapper, submitKey, answer } = mountEntry()
    await wrapper.find('input').setValue('the key')
    await wrapper.find('form').trigger('submit')
    expect(submitKey).toHaveBeenCalledWith('the key')
    expect(wrapper.text()).toContain('Checking the key…')
    expect(wrapper.find('button').attributes('disabled')).toBeDefined()

    answer(false)
    await flushPromises()
    expect(wrapper.text()).not.toContain('Checking the key…')
    expect(wrapper.text()).toContain('The key was not accepted.')
    expect((wrapper.find('input').element as HTMLInputElement).value).toBe('the key')
  })

  it('does not submit an empty key', async () => {
    const { wrapper, submitKey } = mountEntry()
    await wrapper.find('form').trigger('submit')
    expect(submitKey).not.toHaveBeenCalled()
  })

  it('speaks Chinese', () => {
    const { wrapper } = mountEntry('zh-CN')
    expect(wrapper.find('h1').text()).toBe('输入密钥')
  })
})
