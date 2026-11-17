import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { describe, expect, it, vi } from 'vitest'
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

  it('places tiles only once the observer reports the area size, never on the 0×0 grid', async () => {
    // A tile mounted before the area is measured gets the degenerate one-column layout and then
    // animates left/top/width/height across the content area — the jank of the split toggle.
    const observers: ResizeObserverCallback[] = []
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: ResizeObserverCallback) {
          observers.push(callback)
        }
        observe() {}
        disconnect() {}
      },
    )
    try {
      const pinia = createPinia()
      setActivePinia(pinia)
      const workspace = useWorkspaceStore()
      workspace.onFrame({
        kind: 'sessions',
        sessions: [
          info({ id: 's1', name: 'main' }),
          info({ id: 's2', name: 'bg', created_at: 2, attached: 0 }),
          info({ id: 's3', name: 'bg2', created_at: 3, attached: 0 }),
        ],
      })
      workspace.currentId = 's1'
      workspace.mode = 'split'

      const wrapper = mount(SplitView, {
        global: { plugins: [pinia, createAppI18n(['en'])], stubs: { SplitTile: TileStub } },
      })
      await nextTick()
      expect(wrapper.findAll('[data-tile]')).toHaveLength(0)

      const entry = { contentRect: { width: 1200, height: 600 } } as ResizeObserverEntry
      observers[0]!([entry], {} as ResizeObserver)
      await nextTick()
      const tiles = wrapper.findAll('[data-tile]')
      expect(tiles).toHaveLength(2)
      expect(tiles[0]!.attributes('style')).toContain('left: 16px')
      expect(tiles[0]!.attributes('style')).toContain('width: 578px')
      expect(tiles[1]!.attributes('style')).toContain('left: 606px')
    } finally {
      vi.unstubAllGlobals()
    }
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

  it('throttles scroll clearance measurements to one requestAnimationFrame and cancels on unmount', async () => {
    const rafQueue: FrameRequestCallback[] = []
    let nextRafId = 1
    const rafSpy = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => {
      rafQueue.push(cb)
      return nextRafId++
    })
    const cancelSpy = vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {})

    try {
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

      const wrapper = mount(SplitView, {
        global: { plugins: [pinia, createAppI18n(['en'])], stubs: { SplitTile: TileStub } },
      })
      await nextTick()
      rafQueue.length = 0
      rafSpy.mockClear()

      const splitEl = wrapper.get('.split').element as HTMLElement
      const rectSpy = vi.spyOn(splitEl, 'getBoundingClientRect')

      await wrapper.get('.split').trigger('scroll')
      await wrapper.get('.split').trigger('scroll')
      await wrapper.get('.split').trigger('scroll')

      expect(rafSpy).toHaveBeenCalledTimes(1)
      expect(rectSpy).not.toHaveBeenCalled()

      rafQueue.shift()!(16)
      expect(rectSpy).toHaveBeenCalledTimes(1)

      const unmountId = nextRafId
      await wrapper.get('.split').trigger('scroll')
      expect(rafSpy).toHaveBeenCalledTimes(2)
      wrapper.unmount()
      expect(cancelSpy).toHaveBeenCalledWith(unmountId)
    } finally {
      rafSpy.mockRestore()
      cancelSpy.mockRestore()
    }
  })

  it('keeps TransitionGroup grid mounted when empty and freezes focus on leave', async () => {
    const observers: ResizeObserverCallback[] = []
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: ResizeObserverCallback) {
          observers.push(callback)
        }
        observe() {}
        disconnect() {}
      },
    )
    try {
      const pinia = createPinia()
      setActivePinia(pinia)
      const workspace = useWorkspaceStore()
      workspace.onFrame({
        kind: 'sessions',
        sessions: [info({ id: 's2', name: 'bg', created_at: 2, attached: 0 })],
      })
      workspace.mode = 'split'
      workspace.focusTile('s2')

      const wrapper = mount(SplitView, {
        global: { plugins: [pinia, createAppI18n(['en'])], stubs: { SplitTile: TileStub } },
      })
      observers[0]!(
        [{ contentRect: { width: 900, height: 600 } } as ResizeObserverEntry],
        {} as ResizeObserver,
      )
      await nextTick()
      expect(wrapper.get('[data-tile="s2"]').classes()).toContain('focused')
      expect(wrapper.find('.grid').exists()).toBe(true)

      // Leaving split clears store focus, but leaving SplitView keeps visual focus frozen.
      workspace.mode = 'single'
      workspace.focusTile(undefined)
      await nextTick()
      expect(wrapper.get('[data-tile="s2"]').classes()).toContain('focused')

      // Re-enter split and remove last tile: .grid remains mounted alongside .empty.
      workspace.mode = 'split'
      workspace.onFrame({ kind: 'sessions', sessions: [] })
      await nextTick()
      expect(wrapper.find('.grid').exists()).toBe(true)
      expect(wrapper.find('.empty').exists()).toBe(true)
      wrapper.unmount()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('tweens scrollTop to 0 when zooming a tile from a scrolled grid', async () => {
    const rafQueue: FrameRequestCallback[] = []
    const rafSpy = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => {
      rafQueue.push(cb)
      return rafQueue.length
    })
    try {
      const pinia = createPinia()
      setActivePinia(pinia)
      const workspace = useWorkspaceStore()
      workspace.onFrame({
        kind: 'sessions',
        sessions: [info({ id: 's2', name: 'bg', created_at: 2, attached: 0 })],
      })
      workspace.mode = 'split'
      const wrapper = mount(SplitView, {
        global: { plugins: [pinia, createAppI18n(['en'])], stubs: { SplitTile: TileStub } },
      })
      const splitEl = wrapper.get('.split').element as HTMLElement
      splitEl.scrollTop = 240
      workspace.zoomTile('s2')
      await nextTick()
      // Advance frameLoop past motion.slow (480ms).
      const first = rafQueue.shift()
      first?.(100)
      const second = rafQueue.shift()
      second?.(700)
      expect(splitEl.scrollTop).toBe(0)
      wrapper.unmount()
    } finally {
      rafSpy.mockRestore()
    }
  })
})
