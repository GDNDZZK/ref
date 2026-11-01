import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import { info } from '../model/testMessages.ts'
import type { Todo } from '../proto/generated'
import { useWorkspaceStore } from '../stores/workspace.ts'
import Sidebar from './Sidebar.vue'

let pinia = createPinia()

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
})

function mountSidebar() {
  const workspace = useWorkspaceStore()
  workspace.onFrame({
    kind: 'sessions',
    sessions: [
      info({ id: 's1', name: 'alpha' }),
      info({ id: 's2', name: 'beta', created_at: 2, attached: 0 }),
    ],
  })
  const wrapper = mount(Sidebar, {
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
    workspace.tileFocusId = 's1'
    await nextTick()
    expect(marked(wrapper)).toEqual(['alpha'])

    workspace.focusTile('s2')
    await nextTick()
    expect(marked(wrapper)).toEqual(['beta'])
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
      workspace.tileFocusId = 's1'
      await nextTick()
      expect(wrapper.get('.todo-section .heading').text()).toBe('Todo 1/2')

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
