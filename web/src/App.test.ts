import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import App from './App.vue'
import { createAppI18n } from './i18n/index.ts'
import { useConnectionStore } from './stores/connection.ts'
import { useDialogsStore } from './stores/dialogs.ts'
import { useWorkspaceStore } from './stores/workspace.ts'

describe('App empty workspace state', () => {
  beforeEach(() => {
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: () => {} }))
  })

  function mountApp() {
    const pinia = createPinia()
    setActivePinia(pinia)
    const connection = useConnectionStore()
    const workspace = useWorkspaceStore()
    const dialogs = useDialogsStore()
    connection.state = 'open'
    workspace.cwd = '/home/user/project'
    const wrapper = mount(App, {
      global: {
        plugins: [pinia, createAppI18n(['en'])],
        stubs: {
          Sidebar: true,
          SplitView: true,
          SessionPane: true,
          DialogHost: true,
        },
      },
    })
    return { wrapper, connection, workspace, dialogs }
  }

  it('renders centered empty workspace actions and cwd when connected with no open session', async () => {
    const { wrapper, workspace, dialogs } = mountApp()
    const newSpy = vi.spyOn(workspace, 'newSession').mockResolvedValue('ses_1')

    const empty = wrapper.get('.empty-workspace')
    expect(empty.text()).toContain('No session open')
    expect(empty.text()).toContain('/home/user/project')

    const buttons = empty.findAll('button')
    expect(buttons.map((b) => b.text())).toEqual(['New session in folder…', 'New session'])

    await buttons[0]!.trigger('click')
    expect(dialogs.current).toEqual({ kind: 'newSession' })

    await buttons[1]!.trigger('click')
    expect(newSpy).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })

  it('hides the empty workspace state while connecting or when in split view', async () => {
    const { wrapper, connection, workspace } = mountApp()
    connection.state = 'connecting'
    await nextTick()
    expect(wrapper.find('.empty-workspace').exists()).toBe(false)

    connection.state = 'open'
    workspace.mode = 'split'
    await nextTick()
    expect(wrapper.find('.empty-workspace').exists()).toBe(false)
    wrapper.unmount()
  })

  it('highlights the split button in split mode and shows a dedicated unzoom button when zoomed', async () => {
    const { wrapper, workspace } = mountApp()
    expect(wrapper.find('.split-button').classes()).not.toContain('active')
    expect(wrapper.find('.unzoom-button').exists()).toBe(false)

    workspace.mode = 'split'
    workspace.zoomedId = 'ses_2'
    await nextTick()
    expect(wrapper.find('.split-button').classes()).toContain('active')
    const unzoomBtn = wrapper.get('.unzoom-button')
    await unzoomBtn.trigger('click')
    expect(workspace.zoomedId).toBeUndefined()
    await nextTick()
    expect(wrapper.find('.unzoom-button').exists()).toBe(false)
    wrapper.unmount()
  })
})
