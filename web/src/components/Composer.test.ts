import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import Composer from './Composer.vue'

function mountComposer(busy = false) {
  return mount(Composer, { props: { busy }, global: { plugins: [createAppI18n(['en'])] } })
}

async function type(wrapper: ReturnType<typeof mountComposer>, text: string) {
  await wrapper.get('textarea').setValue(text)
}

describe('Composer', () => {
  it('sends on Enter and clears the input', async () => {
    const wrapper = mountComposer()
    await type(wrapper, 'hello')
    await wrapper.get('textarea').trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('submitted')).toEqual([['hello']])
    expect((wrapper.get('textarea').element as HTMLTextAreaElement).value).toBe('')
  })

  it('keeps the line break on Shift+Enter', async () => {
    const wrapper = mountComposer()
    await type(wrapper, 'hello')
    await wrapper.get('textarea').trigger('keydown', { key: 'Enter', shiftKey: true })
    expect(wrapper.emitted('submitted')).toBeUndefined()
  })

  it('does not send while an input method is composing', async () => {
    const wrapper = mountComposer()
    await type(wrapper, 'ni')
    await wrapper.get('textarea').trigger('keydown', { key: 'Enter', isComposing: true })
    expect(wrapper.emitted('submitted')).toBeUndefined()
  })

  it('does not send blank text', async () => {
    const wrapper = mountComposer()
    await type(wrapper, '  \n ')
    await wrapper.get('textarea').trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('submitted')).toBeUndefined()
  })

  it('asks to stop, not to send, from the button while busy', async () => {
    const wrapper = mountComposer(true)
    await wrapper.get('button.send').trigger('click')
    expect(wrapper.emitted('stopRequested')).toHaveLength(1)
    expect(wrapper.emitted('submitted')).toBeUndefined()
  })
})
