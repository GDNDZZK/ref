import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import { info } from '../model/testMessages.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import SplitView from './SplitView.vue'

const TileStub = {
  props: ['session', 'focused'],
  template: '<div class="tile" :data-tile="session.id" :class="{ focused }" />',
}

describe('SplitView', () => {
  it('shows the empty state when only the hidden foreground session is live', () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const workspace = useWorkspaceStore()
    workspace.onFrame({
      kind: 'sessions',
      sessions: [info({ id: 's1', name: 'main' })],
    })
    workspace.currentId = 's1'
    workspace.mode = 'split'

    const wrapper = mount(SplitView, {
      global: { plugins: [pinia, createAppI18n(['en'])], stubs: { SplitTile: TileStub } },
    })
    expect(wrapper.find('.empty').exists()).toBe(true)
    expect(wrapper.findAll('[data-tile]')).toHaveLength(0)
  })

  it('clears the focused tile when clicking the grid background', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const workspace = useWorkspaceStore()
    workspace.onFrame({
      kind: 'sessions',
      sessions: [
        info({ id: 's1', name: 'main' }),
        info({ id: 's2', name: 'bg', created_at: 2, attached: 0 }),
      ],
    })
    workspace.currentId = 's1'
    workspace.mode = 'split'
    workspace.focusTile('s2')

    const wrapper = mount(SplitView, {
      global: { plugins: [pinia, createAppI18n(['en'])], stubs: { SplitTile: TileStub } },
    })
    await nextTick()
    expect(workspace.tileFocusId).toBe('s2')

    await wrapper.get('.grid').trigger('click')
    expect(workspace.tileFocusId).toBeUndefined()
  })
})
