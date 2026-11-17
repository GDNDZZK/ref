import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import { info } from '../model/testMessages.ts'
import type { Todo } from '../proto/generated'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import Sidebar from './Sidebar.vue'

let pinia = createPinia()

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
})
afterEach(() => {
  document.body.innerHTML = ''
})

function mountSidebar() {
  const workspace = useWorkspaceStore()
  workspace.onFrame({
    kind: 'sessions',
    sessions: [
      info({ id: 's1', name: 'alpha' }),
      info({ id: 's2', name: 'beta', created_at: 2, attached: 0 }),
      info({ id: 's3', name: 'gamma', created_at: 3, attached: 0 }),
    ],
  })
  const wrapper = mount(Sidebar, {
    attachTo: document.body,
    global: { plugins: [pinia, createAppI18n(['en'])] },
  })
  return { workspace, wrapper }
}

const marked = (wrapper: ReturnType<typeof mountSidebar>['wrapper']) =>
  wrapper.findAll('.running .row.current .title').map((title) => title.text())

describe('Sidebar', () => {
  it('marks the focused tile in split view, not the session split view returns to', async () => {
    const { workspace, wrapper } = mountSidebar()
    workspace.currentId = 's1'
    workspace.mode = 'split'
    await nextTick()
    expect(marked(wrapper)).toEqual([])

    workspace.focusTile('s2')
    await nextTick()
    expect(marked(wrapper)).toEqual(['beta'])

    workspace.focusTile('s3')
    await nextTick()
    expect(marked(wrapper)).toEqual(['gamma'])
  })
  it('detaches the active session when clicking the already-current row in single view', async () => {
    const { workspace, wrapper } = mountSidebar()
    const detachSpy = vi.spyOn(workspace, 'detachCurrent').mockResolvedValue(undefined)
    const openSpy = vi.spyOn(workspace, 'openSession').mockResolvedValue(undefined)
    workspace.currentId = 's1'
    await nextTick()

    const rows = wrapper.findAll('.running .row')
    await rows[0]!.trigger('click')
    expect(detachSpy).toHaveBeenCalledTimes(1)
    expect(openSpy).not.toHaveBeenCalled()

    await rows[1]!.trigger('click')
    expect(openSpy).toHaveBeenCalledWith('s2')
    wrapper.unmount()
  })

  it('shows Move to background in context menu only for the current row in single view', async () => {
    const { workspace, wrapper } = mountSidebar()
    const detachSpy = vi.spyOn(workspace, 'detachCurrent').mockResolvedValue(undefined)
    workspace.currentId = 's1'
    await nextTick()

    const rows = wrapper.findAll('.running .row')
    await rows[0]!.trigger('contextmenu')
    const currentMenu = [...document.body.querySelectorAll('[role="menuitem"]')].map((el) =>
      el.textContent?.trim(),
    )
    expect(currentMenu).toEqual(['Rename', 'Move to background', 'Close'])
    ;(document.body.querySelectorAll('[role="menuitem"]')[1] as HTMLElement).click()
    expect(detachSpy).toHaveBeenCalledTimes(1)
    await nextTick()

    await rows[1]!.trigger('contextmenu')
    const otherMenu = [...document.body.querySelectorAll('[role="menuitem"]')].map((el) =>
      el.textContent?.trim(),
    )
    expect(otherMenu).toEqual(['Rename', 'Close'])
    wrapper.unmount()
  })

  it('confirms before closing a busy session and closes an idle session directly', async () => {
    const { workspace, wrapper } = mountSidebar()
    const dialogs = useDialogsStore()
    const closeSpy = vi.spyOn(workspace, 'closeSession').mockResolvedValue(undefined)
    workspace.onFrame({
      kind: 'sessions',
      sessions: [
        info({ id: 's1', name: 'busy-one', status: { state: 'busy' } }),
        info({ id: 's2', name: 'idle-one', created_at: 2 }),
      ],
    })
    await nextTick()

    const closeButtons = wrapper.findAll('.running .row button.close')
    await closeButtons[0]!.trigger('click')
    expect(dialogs.current).toEqual({ kind: 'closeSession', sessionId: 's1' })
    expect(closeSpy).not.toHaveBeenCalled()

    dialogs.close()
    await closeButtons[1]!.trigger('click')
    expect(dialogs.current).toBeUndefined()
    expect(closeSpy).toHaveBeenCalledWith('s2')
    wrapper.unmount()
  })

  describe('Todo section', () => {
    const todos = (...statuses: Todo['status'][]): Todo[] =>
      statuses.map((status, index) => ({ content: `step ${index + 1}`, status }))

    function mountWithTodos() {
      const { workspace, wrapper } = mountSidebar()
      const sessions = (first: Todo[], second: Todo[]) => [
        info({ id: 's1', name: 'alpha', todos: first }),
        info({ id: 's2', name: 'beta', created_at: 2, attached: 0, todos: second }),
      ]
      workspace.onFrame({
        kind: 'sessions',
        sessions: sessions(todos('completed', 'in_progress', 'pending', 'cancelled'), []),
      })
      workspace.currentId = 's1'
      return { workspace, wrapper, sessions }
    }

    it('lists the todos of the session on screen with done/total, cancelled counted in neither', async () => {
      const { wrapper } = mountWithTodos()
      await nextTick()
      const section = wrapper.get('.todo-section')
      expect(section.get('.heading').text()).toBe('Todo 1/3')
      expect(section.findAll('.todo').map((row) => row.text())).toEqual([
        'step 1',
        'step 2',
        'step 3',
        'step 4',
      ])
    })

    it('is hidden while the session has no todos', async () => {
      const { workspace, wrapper } = mountWithTodos()
      workspace.currentId = 's2'
      await nextTick()
      expect(wrapper.find('.todo-section').exists()).toBe(false)
    })

    it('follows the focused tile in split view', async () => {
      const { workspace, wrapper, sessions } = mountWithTodos()
      workspace.onFrame({
        kind: 'sessions',
        sessions: sessions(todos('completed', 'in_progress'), todos('pending', 'pending', 'pending')),
      })
      workspace.mode = 'split'
      workspace.focusTile('s2')
      await nextTick()
      expect(wrapper.get('.todo-section .heading').text()).toBe('Todo 0/3')
    })

    it('updates when the list changes', async () => {
      const { workspace, wrapper, sessions } = mountWithTodos()
      await nextTick()
      workspace.onFrame({
        kind: 'sessions',
        sessions: sessions(todos('completed', 'completed', 'completed'), []),
      })
      await nextTick()
      expect(wrapper.get('.todo-section .heading').text()).toBe('Todo 3/3')
    })
  })
})
