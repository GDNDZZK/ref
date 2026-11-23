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
    expect(currentMenu).toEqual(['Rename', 'Move to background', 'Filter spawned history', 'Close'])
    ;(document.body.querySelectorAll('[role="menuitem"]')[1] as HTMLElement).click()
    expect(detachSpy).toHaveBeenCalledTimes(1)
    await nextTick()

    await rows[1]!.trigger('contextmenu')
    const otherMenu = [...document.body.querySelectorAll('[role="menuitem"]')].map((el) =>
      el.textContent?.trim(),
    )
    expect(otherMenu).toEqual(['Rename', 'Filter spawned history', 'Close'])
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

  describe('vertical section resizers', () => {
    function mockVerticalRects(heights: { body: number; lists: number; todo: number; running: number }) {
      const origRect = HTMLElement.prototype.getBoundingClientRect
      return vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
        this: HTMLElement,
      ) {
        const makeRect = (height: number): DOMRect =>
          ({
            top: 0,
            left: 0,
            x: 0,
            y: 0,
            width: 248,
            height,
            right: 248,
            bottom: height,
            toJSON: () => ({}),
          }) as DOMRect
        if (this.classList.contains('body')) return makeRect(heights.body)
        if (this.classList.contains('lists')) return makeRect(heights.lists)
        if (this.classList.contains('todo-slot')) return makeRect(heights.todo)
        if (this.classList.contains('running-slot')) return makeRect(heights.running)
        return origRect.call(this)
      })
    }

    it('drags Todo<->Running and Running<->History resizers, clamps, persists, and resets on double-click', async () => {
      const rectSpy = mockVerticalRects({ body: 600, lists: 440, todo: 120, running: 160 })
      try {
        const { workspace, wrapper } = mountSidebar()
        const layout = useLayoutStore()
        if (!layout.sidebarVisible) layout.toggleSidebar()
        workspace.onFrame({
          kind: 'sessions',
          sessions: [info({ id: 's1', name: 'alpha', todos: [{ content: 'step 1', status: 'pending' }] })],
        })
        workspace.currentId = 's1'
        await nextTick()

        const aside = wrapper.get('aside.sidebar')
        const splitters = wrapper.findAll('.section-resizer[role="separator"][aria-orientation="horizontal"]')
        expect(splitters).toHaveLength(2)
        const [todoSplitter, runningSplitter] = splitters as [
          (typeof splitters)[number],
          (typeof splitters)[number],
        ]

        // Drag Todo <-> Running from auto (measured 120px) down by 80px -> 200px.
        await todoSplitter.trigger('pointerdown', { button: 0, pointerId: 1, clientY: 120 })
        expect(aside.classes()).toContain('resizing-sections')
        expect(todoSplitter.classes()).toContain('active')
        await todoSplitter.trigger('pointermove', { pointerId: 1, clientY: 200 })
        await nextTick()
        expect(layout.sidebarTodoHeight).toBe(200)
        expect(wrapper.get('.todo-slot').classes()).toContain('custom-height')
        expect(wrapper.get('.todo-slot').attributes('style')).toContain('--todo-height: 200px')
        expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({ sidebarTodoHeight: 200 })

        // Clamps to min (66px) and max (600 - (28*2 + 6 + 56 + 6 + 32 + 4 + 44) = 396px).
        await todoSplitter.trigger('pointermove', { pointerId: 1, clientY: -200 })
        expect(layout.sidebarTodoHeight).toBe(66)
        await todoSplitter.trigger('pointermove', { pointerId: 1, clientY: 900 })
        expect(layout.sidebarTodoHeight).toBe(396)
        await todoSplitter.trigger('pointerup', { pointerId: 1 })
        expect(aside.classes()).not.toContain('resizing-sections')

        // Double-click resets Todo height back to 0 (auto).
        await todoSplitter.trigger('dblclick')
        await nextTick()
        expect(layout.sidebarTodoHeight).toBe(0)
        expect(wrapper.get('.todo-slot').classes()).not.toContain('custom-height')

        // Drag Running <-> History from auto (measured 160px) down by 50px -> 210px.
        await runningSplitter.trigger('pointerdown', { button: 0, pointerId: 2, clientY: 300 })
        expect(aside.classes()).toContain('resizing-sections')
        expect(runningSplitter.classes()).toContain('active')
        await runningSplitter.trigger('pointermove', { pointerId: 2, clientY: 350 })
        await nextTick()
        expect(layout.sidebarRunningHeight).toBe(210)
        expect(wrapper.get('.running-slot').classes()).toContain('custom-height')
        expect(wrapper.get('.running-slot').attributes('style')).toContain('--running-height: 210px')
        expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({ sidebarRunningHeight: 210 })

        // Clamps to min (56px) and max (440 - (28*2 + 6 + 32 + 4 + 44) = 298px).
        await runningSplitter.trigger('pointermove', { pointerId: 2, clientY: -100 })
        expect(layout.sidebarRunningHeight).toBe(metrics.sidebarRowHeight)
        await runningSplitter.trigger('pointermove', { pointerId: 2, clientY: 900 })
        expect(layout.sidebarRunningHeight).toBe(298)
        await runningSplitter.trigger('pointerup', { pointerId: 2 })
        expect(aside.classes()).not.toContain('resizing-sections')

        // Double-click resets Running height back to 0 (auto).
        await runningSplitter.trigger('dblclick')
        await nextTick()
        expect(layout.sidebarRunningHeight).toBe(0)
        expect(wrapper.get('.running-slot').classes()).not.toContain('custom-height')
        wrapper.unmount()
      } finally {
        rectSpy.mockRestore()
      }
    })

    it('works in narrow drawer mode and cleans up drag state on pointercancel, lostpointercapture, and collapse', async () => {
      const rectSpy = mockVerticalRects({ body: 500, lists: 380, todo: 100, running: 140 })
      try {
        const { workspace, wrapper } = mountSidebar()
        const layout = useLayoutStore()
        layout.narrow = true
        layout.toggleSidebar() // opens drawer in narrow mode
        workspace.onFrame({
          kind: 'sessions',
          sessions: [info({ id: 's1', name: 'alpha', todos: [{ content: 'step 1', status: 'pending' }] })],
        })
        workspace.currentId = 's1'
        await nextTick()

        const aside = wrapper.get('aside.sidebar')
        const splitters = wrapper.findAll('.section-resizer')
        expect(splitters).toHaveLength(2)
        const todoSplitter = splitters[0]!
        const runningSplitter = splitters[1]!
        await todoSplitter.trigger('pointerdown', { button: 0, pointerId: 1, clientY: 100 })
        expect(aside.classes()).toContain('resizing-sections')
        await todoSplitter.trigger('pointercancel', { pointerId: 1 })
        expect(aside.classes()).not.toContain('resizing-sections')

        // lostpointercapture clears resizing-sections.
        await runningSplitter.trigger('pointerdown', { button: 0, pointerId: 2, clientY: 200 })
        expect(aside.classes()).toContain('resizing-sections')
        await runningSplitter.trigger('lostpointercapture', { pointerId: 2 })
        expect(aside.classes()).not.toContain('resizing-sections')

        // Collapsing History mid-drag hides Running<->History splitter and cleans up drag state.
        await runningSplitter.trigger('pointerdown', { button: 0, pointerId: 3, clientY: 200 })
        expect(aside.classes()).toContain('resizing-sections')
        layout.toggleSidebarHistory()
        await nextTick()
        expect(aside.classes()).not.toContain('resizing-sections')
        expect(wrapper.findAll('.section-resizer')).toHaveLength(1) // only Todo<->Running remains

        // Collapsing Running as well hides Todo<->Running splitter too.
        layout.toggleSidebarRunning()
        await nextTick()
        expect(wrapper.findAll('.section-resizer')).toHaveLength(0)
        wrapper.unmount()
      } finally {
        rectSpy.mockRestore()
      }
    })
  })

  describe('smooth VS Code accordion collapse/expand', () => {
    it('keeps custom-height class and CSS height vars when collapsed so height transitions smoothly', async () => {
      const { workspace, wrapper } = mountSidebar()
      const layout = useLayoutStore()
      if (!layout.sidebarVisible) layout.toggleSidebar()
      workspace.onFrame({
        kind: 'sessions',
        sessions: [info({ id: 's1', name: 'alpha', todos: [{ content: 'step 1', status: 'pending' }] })],
      })
      workspace.currentId = 's1'
      layout.setSidebarTodoHeight(140)
      layout.setSidebarRunningHeight(200)
      await nextTick()

      const todoSlot = wrapper.get('.todo-slot')
      const runningSlot = wrapper.get('.running-slot')
      const body = wrapper.get('.body')
      const lists = wrapper.get('.lists')

      expect(todoSlot.classes()).toContain('open')
      expect(todoSlot.classes()).toContain('custom-height')
      expect(runningSlot.classes()).toContain('open')
      expect(runningSlot.classes()).toContain('custom-height')

      // Collapsing Todo and Running keeps custom-height so CSS transitions customHeight <-> collapsed height.
      layout.toggleSidebarTodo()
      layout.toggleSidebarRunning()
      await nextTick()
      expect(todoSlot.classes()).not.toContain('open')
      expect(todoSlot.classes()).toContain('custom-height')
      expect(todoSlot.attributes('style')).toContain('--todo-height: 140px')
      expect(runningSlot.classes()).not.toContain('open')
      expect(runningSlot.classes()).toContain('custom-height')
      expect(runningSlot.attributes('style')).toContain('--running-height: 200px')

      // Expanding Running while collapsing History lets Running fill remaining height cleanly.
      layout.toggleSidebarRunning()
      layout.toggleSidebarHistory()
      await nextTick()
      expect(lists.classes()).toContain('history-collapsed')
      expect(runningSlot.classes()).toContain('open')
      expect(runningSlot.classes()).toContain('custom-height')

      // Expanding Todo and collapsing both Running & History gives Todo .fill to occupy available space.
      layout.toggleSidebarTodo()
      layout.toggleSidebarRunning()
      await nextTick()
      expect(body.classes()).toContain('lists-collapsed')
      expect(todoSlot.classes()).toContain('open')
      expect(todoSlot.classes()).toContain('fill')
      wrapper.unmount()
    })
  })

  describe('history filter and creator display', () => {
    it('shows creator on HistoryRow, filters via popover, persists settings, and debounces backend re-query', async () => {
      vi.useFakeTimers()
      const { workspace, wrapper } = mountSidebar()
      const layout = useLayoutStore()
      const refreshSpy = vi.spyOn(workspace, 'refreshSidebarHistory').mockResolvedValue(undefined)
      workspace.cwd = '/proj'
      workspace.onFrame({
        kind: 'sessions',
        sessions: [info({ id: 's1', name: 'lead', cwd: '/proj' })],
      })
      workspace.currentId = 's1'
      workspace.sidebarHistoryLoaded = true
      workspace.sidebarHistory = [
        info({ id: 'h1', name: 'user-hist', title: 'User work', cwd: '/proj', parent: null, live: false }),
        info({ id: 'h2', name: 'child-hist', title: 'Child work', cwd: '/other', parent: 's1', live: false }),
      ]
      workspace.history = workspace.sidebarHistory
      await nextTick()

      // Second row displays · ⟲ lead on subtitle.
      const subtitles = wrapper.findAll('.history-row .subtitle').map((el) => el.text())
      expect(subtitles[0]).not.toContain('⟲')
      expect(subtitles[1]).toContain('· ⟲ lead')

      // Filter button opens popover with All, User-created, and Only current directory.
      const filterBtn = wrapper.get('button.filter-button')
      expect(filterBtn.classes()).not.toContain('active')
      await filterBtn.trigger('click')
      await nextTick()
      const items = [...document.body.querySelectorAll('[role="menuitem"]')]
      expect(items.map((el) => el.textContent?.trim())).toEqual([
        'All',
        'User-created',
        'Only current directory',
      ])

      // Pick User-created: persists, highlights filter button, filters locally, and triggers refreshSidebarHistory.
      ;(items[1] as HTMLElement).click()
      await nextTick()
      expect(layout.sidebarHistoryCreatorFilter).toBe('user')
      expect(filterBtn.classes()).toContain('active')
      expect(wrapper.findAll('.history-row .title').map((el) => el.text())).toEqual(['User work'])
      expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({
        sidebarHistoryCreatorFilter: 'user',
      })
      expect(refreshSpy).toHaveBeenCalledTimes(1)
      // Unfiltered workspace.history stays untouched.
      expect(workspace.history).toHaveLength(2)

      // Switch back to All, then toggle Only current directory.
      await filterBtn.trigger('click')
      await nextTick()
      ;(document.body.querySelectorAll('[role="menuitem"]')[0] as HTMLElement).click()
      await nextTick()
      await filterBtn.trigger('click')
      await nextTick()
      ;(document.body.querySelectorAll('[role="menuitem"]')[2] as HTMLElement).click()
      await nextTick()
      expect(layout.sidebarHistoryCurrentDir).toBe(true)
      expect(wrapper.findAll('.history-row .title').map((el) => el.text())).toEqual(['User work'])
      expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({ sidebarHistoryCurrentDir: true })
      expect(refreshSpy).toHaveBeenCalledTimes(3)

      // Search input debounces 150ms before re-querying backend.
      refreshSpy.mockClear()
      const input = wrapper.get('.search input')
      await input.setValue('User')
      expect(refreshSpy).not.toHaveBeenCalled()
      vi.advanceTimersByTime(150)
      expect(refreshSpy).toHaveBeenCalledTimes(1)
      wrapper.unmount()
    })

    it('keeps server-matched child rows when sidebarHistorySynced is true even if parent is absent from byId', async () => {
      const { workspace, wrapper } = mountSidebar()
      workspace.sidebarHistoryLoaded = true
      workspace.sidebarHistory = [
        info({ id: 'h_child', name: 'child-only', title: 'Child task', parent: 's_missing', live: false }),
      ]
      workspace.historyQuery = 'OrchestratorRoot'
      workspace.sidebarHistorySynced = true
      await nextTick()
      expect(wrapper.findAll('.history-row .title').map((el) => el.text())).toEqual(['Child task'])
      wrapper.unmount()
    })

    it('sets ephemeral By <creator> filter from HistoryRow and SidebarRow context menus', async () => {
      const { workspace, wrapper } = mountSidebar()
      const refreshSpy = vi.spyOn(workspace, 'refreshSidebarHistory').mockResolvedValue(undefined)
      workspace.sidebarHistoryLoaded = true
      workspace.sidebarHistory = [
        info({ id: 'h1', name: 'root-h', title: 'Root Hist', parent: null, live: false }),
        info({ id: 'h2', name: 'sub-h', title: 'Sub Hist', parent: 's1', live: false }),
      ]
      await nextTick()

      // Right-click HistoryRow with parent -> Filter history by creator.
      const histRows = wrapper.findAll('.history-row')
      await histRows[1]!.trigger('contextmenu')
      await nextTick()
      const histMenu = [...document.body.querySelectorAll('[role="menuitem"]')]
      expect(histMenu.map((el) => el.textContent?.trim())).toEqual([
        'Filter history by creator',
        'Filter spawned history',
        'Delete',
      ])
      ;(histMenu[0] as HTMLElement).click()
      await nextTick()
      expect(workspace.historyCreator).toBe('s1')
      expect(wrapper.findAll('.history-row .title').map((el) => el.text())).toEqual(['Sub Hist'])
      expect(refreshSpy).toHaveBeenCalledTimes(1)

      // Filter popover now shows "By alpha" checked.
      await wrapper.get('button.filter-button').trigger('click')
      await nextTick()
      const popoverItems = [...document.body.querySelectorAll('[role="menuitem"]')]
      expect(popoverItems.map((el) => el.textContent?.trim())).toEqual([
        'All',
        'User-created',
        'By alpha',
        'Only current directory',
      ])
      expect(popoverItems[2]!.getAttribute('aria-checked')).toBe('true')
      // Close menu, then right-click SidebarRow s2 -> Filter spawned history.
      ;(popoverItems[0] as HTMLElement).click()
      await nextTick()
      await wrapper.findAll('.running .row')[1]!.trigger('contextmenu')
      await nextTick()
      const runningMenu = [...document.body.querySelectorAll('[role="menuitem"]')]
      const spawnedItem = runningMenu.find((el) => el.textContent?.trim() === 'Filter spawned history')!
      ;(spawnedItem as HTMLElement).click()
      await nextTick()
      expect(workspace.historyCreator).toBe('s2')
      expect(refreshSpy).toHaveBeenCalledTimes(3)
      wrapper.unmount()
    })
  })
})
