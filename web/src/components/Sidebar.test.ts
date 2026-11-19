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
  vi.useRealTimers()
  Object.defineProperty(window, 'innerWidth', {
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

  describe('running session long-press reorder', () => {
    it('still opens/detaches on a short click without triggering drag', async () => {
      vi.useFakeTimers()
      const { workspace, wrapper } = mountSidebar()
      const detachSpy = vi.spyOn(workspace, 'detachCurrent').mockResolvedValue(undefined)
      const openSpy = vi.spyOn(workspace, 'openSession').mockResolvedValue(undefined)
      workspace.currentId = 's1'
      await nextTick()

      const rows = wrapper.findAll('.running .row')
      await rows[1]!.trigger('pointerdown', { button: 0, pointerId: 1, clientX: 40, clientY: 100 })
      vi.advanceTimersByTime(100)
      await rows[1]!.trigger('pointerup', { pointerId: 1, clientX: 40, clientY: 100 })
      await rows[1]!.trigger('click')
      expect(rows[1]!.classes()).not.toContain('dragging')
      expect(openSpy).toHaveBeenCalledWith('s2')
      expect(detachSpy).not.toHaveBeenCalled()
      wrapper.unmount()
    })

    it('cancels the hold timer when moving more than 6px before 280ms', async () => {
      vi.useFakeTimers()
      const { workspace, wrapper } = mountSidebar()
      await nextTick()

      const row = wrapper.findAll('.running .row')[0]!
      await row.trigger('pointerdown', { button: 0, pointerId: 1, clientX: 40, clientY: 100 })
      await row.trigger('pointermove', { pointerId: 1, clientX: 40, clientY: 108 })
      vi.advanceTimersByTime(300)
      await nextTick()
      expect(row.classes()).not.toContain('dragging')
      await row.trigger('pointermove', { pointerId: 1, clientX: 40, clientY: 240 })
      expect(workspace.live.map((s) => s.id)).toEqual(['s1', 's2', 's3'])
      wrapper.unmount()
    })

    it('reorders rows after holding >=280ms, persists to localStorage, and suppresses click on drop', async () => {
      vi.useFakeTimers()
      const { workspace, wrapper } = mountSidebar()
      const openSpy = vi.spyOn(workspace, 'openSession').mockResolvedValue(undefined)
      const detachSpy = vi.spyOn(workspace, 'detachCurrent').mockResolvedValue(undefined)
      workspace.currentId = 's1'
      await nextTick()

      const firstRow = wrapper.findAll('.running .row')[0]!
      await firstRow.trigger('pointerdown', { button: 0, pointerId: 1, clientX: 40, clientY: 100 })
      vi.advanceTimersByTime(280)
      await nextTick()
      expect(firstRow.classes()).toContain('dragging')

      const stride = metrics.sidebarRowHeight + 2
      await firstRow.trigger('pointermove', { pointerId: 1, clientX: 40, clientY: 100 + stride * 2 })
      await nextTick()
      expect(workspace.live.map((s) => s.id)).toEqual(['s2', 's3', 's1'])
      expect(wrapper.findAll('.running .row .title').map((t) => t.text())).toEqual(['beta', 'gamma', 'alpha'])
      expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({
        sidebarRunningOrder: ['s2', 's3', 's1'],
      })

      await firstRow.trigger('pointerup', { pointerId: 1, clientX: 40, clientY: 100 + stride * 2 })
      await firstRow.trigger('click')
      expect(firstRow.classes()).not.toContain('dragging')
      expect(openSpy).not.toHaveBeenCalled()
      expect(detachSpy).not.toHaveBeenCalled()
      wrapper.unmount()
    })

    it('cleans up dragging state on pointercancel, lostpointercapture, and section collapse', async () => {
      vi.useFakeTimers()
      const { wrapper } = mountSidebar()
      const layout = useLayoutStore()
      await nextTick()

      const row = wrapper.findAll('.running .row')[0]!
      await row.trigger('pointerdown', { button: 0, pointerId: 1, clientX: 40, clientY: 100 })
      vi.advanceTimersByTime(280)
      await nextTick()
      expect(row.classes()).toContain('dragging')
      await row.trigger('pointercancel', { pointerId: 1 })
      await nextTick()
      expect(row.classes()).not.toContain('dragging')

      await row.trigger('pointerdown', { button: 0, pointerId: 2, clientX: 40, clientY: 100 })
      vi.advanceTimersByTime(280)
      await nextTick()
      expect(row.classes()).toContain('dragging')
      await row.trigger('lostpointercapture', { pointerId: 2 })
      await nextTick()
      expect(row.classes()).not.toContain('dragging')

      await row.trigger('pointerdown', { button: 0, pointerId: 3, clientX: 40, clientY: 100 })
      vi.advanceTimersByTime(280)
      await nextTick()
      expect(row.classes()).toContain('dragging')
      layout.toggleSidebarRunning()
      await nextTick()
      expect(row.classes()).not.toContain('dragging')
      wrapper.unmount()
    })

    it('clears .dragging and any pending .session-move class on drag end', async () => {
      vi.useFakeTimers()
      const origGetComputedStyle = window.getComputedStyle.bind(window)
      const styleSpy = vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
        if (el instanceof HTMLElement && el.classList.contains('session-move')) {
          return {
            ...origGetComputedStyle(el, pseudo),
            transitionProperty: 'transform',
            transitionDuration: '0.2s',
            transitionDelay: '0s',
            animationDuration: '0s',
            animationDelay: '0s',
          } as CSSStyleDeclaration
        }
        return origGetComputedStyle(el, pseudo)
      })
      const origRect = HTMLElement.prototype.getBoundingClientRect
      const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
        this: HTMLElement,
      ) {
        if (this.classList.contains('row') && this.parentElement?.classList.contains('running')) {
          const rows = [...this.parentElement.querySelectorAll(':scope > .row')]
          const idx = rows.indexOf(this)
          const stride = metrics.sidebarRowHeight + 2
          const top = Math.max(0, idx) * stride
          return {
            top,
            left: 0,
            x: 0,
            y: top,
            width: 248,
            height: metrics.sidebarRowHeight,
            right: 248,
            bottom: top + metrics.sidebarRowHeight,
            toJSON: () => ({}),
          } as DOMRect
        }
        return origRect.call(this)
      })
      try {
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
          global: {
            plugins: [pinia, createAppI18n(['en'])],
            stubs: { TransitionGroup: false },
          },
        })

        const firstRow = wrapper.findAll('.running .row')[0]!
        await firstRow.trigger('pointerdown', { button: 0, pointerId: 1, clientX: 40, clientY: 20 })
        vi.advanceTimersByTime(280)
        await nextTick()
        expect(firstRow.classes()).toContain('dragging')

        const stride = metrics.sidebarRowHeight + 2
        await firstRow.trigger('pointermove', { pointerId: 1, clientX: 40, clientY: 20 + stride })
        await nextTick()
        expect(workspace.live.map((s) => s.id)).toEqual(['s2', 's1', 's3'])
        expect(wrapper.findAll('.running .row').map((r) => r.classes())).toContainEqual(
          expect.arrayContaining(['dragging', 'session-move']),
        )

        await firstRow.trigger('pointerup', { pointerId: 1, clientX: 40, clientY: 20 + stride })
        await nextTick()
        expect(wrapper.findAll('.running .row.dragging')).toHaveLength(0)
        expect(wrapper.findAll('.running .row.session-move')).toHaveLength(0)
        wrapper.unmount()
      } finally {
        rectSpy.mockRestore()
        styleSpy.mockRestore()
      }
    })
  })
})
