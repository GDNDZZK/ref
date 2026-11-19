import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createAppI18n } from '../i18n/index.ts'
import { info } from '../model/testMessages.ts'
import type { Todo } from '../proto/generated'
import { useDialogsStore } from '../stores/dialogs.ts'
import { useLayoutStore } from '../stores/layout.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'
import { STORAGE_KEY } from '../settings/model.ts'
import { metrics } from '../theme/generated/theme.ts'
import Sidebar from './Sidebar.vue'

let pinia = createPinia()
const originalInnerWidth = window.innerWidth

beforeEach(() => {
  localStorage.clear()
  pinia = createPinia()
  setActivePinia(pinia)
})
afterEach(() => {
  Object.defineProperty(window, 'innerWidth', {
    configurable: true,
    writable: true,
    value: originalInnerWidth,
  })
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

  it('marks sidebar content inert while collapsed so Tab skips hidden controls', async () => {
    const { wrapper } = mountSidebar()
    const layout = useLayoutStore()
    if (!layout.sidebarVisible) layout.toggleSidebar()
    await nextTick()
    expect(wrapper.get('.content').attributes('inert')).toBeUndefined()

    layout.toggleSidebar()
    await nextTick()
    expect(wrapper.get('.content').attributes('inert')).toBeDefined()
    wrapper.unmount()
  })

  it('resizes the sidebar by dragging, clamps to viewport/bounds, persists, and resets on double-click', async () => {
    Object.defineProperty(window, 'innerWidth', { configurable: true, writable: true, value: 1000 })
    const { wrapper } = mountSidebar()
    const layout = useLayoutStore()
    if (!layout.sidebarVisible) layout.toggleSidebar()
    await nextTick()

    const aside = wrapper.get('aside.sidebar')
    expect(aside.attributes('style')).toContain(`--sidebar-width: ${metrics.sidebarWidth}px`)
    const handle = wrapper.get('.resize-handle')

    await handle.trigger('pointerdown', { button: 0, pointerId: 1, clientX: metrics.sidebarWidth })
    expect(aside.classes()).toContain('resizing')
    await handle.trigger('pointermove', { pointerId: 1, clientX: 350 })
    await nextTick()
    expect(layout.sidebarWidth).toBe(350)
    expect(aside.attributes('style')).toContain('--sidebar-width: 350px')
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({ sidebarWidth: 350 })

    // Clamped to min (210) and max (min(480, 1000 * 0.45) = 450).
    await handle.trigger('pointermove', { pointerId: 1, clientX: 800 })
    await nextTick()
    expect(layout.sidebarWidth).toBe(450)
    await handle.trigger('pointermove', { pointerId: 1, clientX: 50 })
    await nextTick()
    expect(layout.sidebarWidth).toBe(metrics.sidebarMinWidth)

    await handle.trigger('pointerup', { pointerId: 1 })
    expect(aside.classes()).not.toContain('resizing')

    // Double-click resets back to default sidebarWidth.
    await handle.trigger('dblclick')
    await nextTick()
    expect(layout.sidebarWidth).toBe(metrics.sidebarWidth)
    expect(aside.attributes('style')).toContain(`--sidebar-width: ${metrics.sidebarWidth}px`)

    // Collapsed or narrow drawer hides the resize handle and omits inline wide width.
    layout.toggleSidebar()
    await nextTick()
    expect(wrapper.find('.resize-handle').exists()).toBe(false)

    layout.narrow = true
    layout.toggleSidebar()
    await nextTick()
    expect(wrapper.find('.resize-handle').exists()).toBe(false)
    expect(aside.attributes('style')).toBeUndefined()
    wrapper.unmount()
  })

  it('resets resizing state on lostpointercapture and when hidden or switched to narrow mid-drag', async () => {
    const { wrapper } = mountSidebar()
    const layout = useLayoutStore()
    if (!layout.sidebarVisible) layout.toggleSidebar()
    await nextTick()

    const aside = wrapper.get('aside.sidebar')
    await wrapper.get('.resize-handle').trigger('pointerdown', { button: 0, pointerId: 1, clientX: 264 })
    expect(aside.classes()).toContain('resizing')
    await wrapper.get('.resize-handle').trigger('lostpointercapture', { pointerId: 1 })
    expect(aside.classes()).not.toContain('resizing')

    // Unmounting the handle mid-drag (collapsing or narrow mode) clears resizing immediately.
    await wrapper.get('.resize-handle').trigger('pointerdown', { button: 0, pointerId: 2, clientX: 264 })
    expect(aside.classes()).toContain('resizing')
    layout.toggleSidebar()
    await nextTick()
    expect(aside.classes()).not.toContain('resizing')
    layout.toggleSidebar()
    await nextTick()
    expect(aside.classes()).not.toContain('resizing')

    await wrapper.get('.resize-handle').trigger('pointerdown', { button: 0, pointerId: 3, clientX: 264 })
    expect(aside.classes()).toContain('resizing')
    layout.narrow = true
    await nextTick()
    expect(aside.classes()).not.toContain('resizing')
    wrapper.unmount()
  })

  it('toggles Todo, Running, and History sections, persists state, and keeps resize working', async () => {
    Object.defineProperty(window, 'innerWidth', { configurable: true, writable: true, value: 1200 })
    const { workspace, wrapper } = mountSidebar()
    const layout = useLayoutStore()
    if (!layout.sidebarVisible) layout.toggleSidebar()
    workspace.onFrame({
      kind: 'sessions',
      sessions: [info({ id: 's1', name: 'alpha', todos: [{ content: 'step 1', status: 'pending' }] })],
    })
    workspace.currentId = 's1'
    await nextTick()

    const toggles = wrapper.findAll('button.section-toggle')
    expect(toggles).toHaveLength(3)
    const [todoToggle, runningToggle, historyToggle] = toggles as [
      (typeof toggles)[number],
      (typeof toggles)[number],
      (typeof toggles)[number],
    ]
    const todoBody = wrapper.get('.todo-body')
    const runningSlot = wrapper.get('.running-slot')
    const historySlot = wrapper.get('.history-slot')
    const lists = wrapper.get('.lists')

    expect(todoToggle.attributes('aria-expanded')).toBe('true')
    expect(runningToggle.attributes('aria-expanded')).toBe('true')
    expect(historyToggle.attributes('aria-expanded')).toBe('true')
    expect(todoBody.attributes('inert')).toBeUndefined()
    expect(runningSlot.attributes('inert')).toBeUndefined()
    expect(historySlot.attributes('inert')).toBeUndefined()
    expect(lists.classes()).not.toContain('history-collapsed')

    await todoToggle.trigger('click')
    expect(todoToggle.attributes('aria-expanded')).toBe('false')
    expect(todoBody.attributes('inert')).toBeDefined()
    expect(todoBody.classes()).not.toContain('open')
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({ sidebarTodoExpanded: false })

    await runningToggle.trigger('click')
    expect(runningToggle.attributes('aria-expanded')).toBe('false')
    expect(runningSlot.attributes('inert')).toBeDefined()
    expect(runningSlot.classes()).not.toContain('open')
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({ sidebarRunningExpanded: false })

    await historyToggle.trigger('click')
    expect(historyToggle.attributes('aria-expanded')).toBe('false')
    expect(historySlot.attributes('inert')).toBeDefined()
    expect(historySlot.classes()).not.toContain('open')
    expect(lists.classes()).toContain('history-collapsed')
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({ sidebarHistoryExpanded: false })

    // Resizing still works when sections are collapsed, and double-click resets width.
    const handle = wrapper.get('.resize-handle')
    await handle.trigger('pointerdown', { button: 0, pointerId: 4, clientX: metrics.sidebarWidth })
    await handle.trigger('pointermove', { pointerId: 4, clientX: 320 })
    await handle.trigger('pointerup', { pointerId: 4 })
    expect(layout.sidebarWidth).toBe(320)
    await handle.trigger('dblclick')
    expect(layout.sidebarWidth).toBe(metrics.sidebarWidth)

    // Expand History back and verify class + inert clear.
    await historyToggle.trigger('click')
    expect(historyToggle.attributes('aria-expanded')).toBe('true')
    expect(historySlot.attributes('inert')).toBeUndefined()
    expect(lists.classes()).not.toContain('history-collapsed')
    wrapper.unmount()
  })
})
