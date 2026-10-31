import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { describe, expect, it } from 'vitest'
import { createAppI18n } from '../i18n/index.ts'
import type { ChatSession } from '../model/chatSession.ts'
import { info } from '../model/testMessages.ts'
import SplitTile from './SplitTile.vue'

const PaneStub = {
  props: ['composerShown'],
  methods: { focusComposer() {} },
  template: '<div class="pane"><button class="allow" /></div>',
}

function mountTile(options: { focused: boolean; zoomed?: boolean }) {
  const session = { id: 's1', info: info({ id: 's1' }) } as unknown as ChatSession
  return mount(SplitTile, {
    props: { session, focused: options.focused, zoomed: options.zoomed ?? false, hidden: false },
    global: { plugins: [createPinia(), createAppI18n(['en'])], stubs: { SessionPane: PaneStub } },
  })
}

describe('SplitTile', () => {
  it('shows the composer only while the tile is focused or zoomed', () => {
    const pane = (wrapper: ReturnType<typeof mountTile>) => wrapper.getComponent(PaneStub)
    expect(pane(mountTile({ focused: false })).props('composerShown')).toBe(false)
    expect(pane(mountTile({ focused: true })).props('composerShown')).toBe(true)
    expect(pane(mountTile({ focused: false, zoomed: true })).props('composerShown')).toBe(true)
  })

  it('takes the focus on a press, but on a button only after its click', async () => {
    const wrapper = mountTile({ focused: false })
    await wrapper.get('.pane').trigger('pointerdown')
    expect(wrapper.emitted('focus')).toHaveLength(1)

    const button = wrapper.get('.allow')
    await button.trigger('pointerdown')
    expect(wrapper.emitted('focus')).toHaveLength(1)
    await button.trigger('click')
    expect(wrapper.emitted('focus')).toHaveLength(2)
  })
})
