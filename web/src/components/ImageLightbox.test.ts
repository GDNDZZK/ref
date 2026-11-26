import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { nextTick, shallowReactive } from 'vue'
import { chatSessionKey } from '../composables/chatSessionContext.ts'
import { createAppI18n } from '../i18n/index.ts'
import type { Block } from '../model/blocks.ts'
import type { ChatSession } from '../model/chatSession.ts'
import ChatView from './ChatView.vue'
import ImageLightbox from './ImageLightbox.vue'

describe('ImageLightbox', () => {
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('prevents wheel events on the lightbox scrim and image from scrolling behind it and zooms', async () => {
    const wrapper = mount(ImageLightbox, {
      props: { url: 'https://example.invalid/pic.png', caption: 'pic' },
      attachTo: document.body,
      global: { plugins: [createAppI18n(['en'])] },
    })

    const scrim = document.body.querySelector('.scrim') as HTMLElement
    const picture = document.body.querySelector('.picture') as HTMLElement

    const scrimWheel = new WheelEvent('wheel', { deltaY: 80, bubbles: true, cancelable: true })
    scrim.dispatchEvent(scrimWheel)
    expect(scrimWheel.defaultPrevented).toBe(true)
    await nextTick()
    expect(document.body.querySelector('.zoom-reset')?.textContent?.trim()).toBe('87%')

    const pictureWheel = new WheelEvent('wheel', { deltaY: -80, bubbles: true, cancelable: true })
    picture.dispatchEvent(pictureWheel)
    expect(pictureWheel.defaultPrevented).toBe(true)
    await nextTick()
    expect(document.body.querySelector('.zoom-reset')?.textContent?.trim()).toBe('100%')

    wrapper.unmount()
  })

  it('supports prev/next buttons, ArrowLeft/ArrowRight keys, counter, and resets zoom on switch', async () => {
    const wrapper = mount(ImageLightbox, {
      props: {
        images: [
          { url: 'https://example.invalid/one.png', caption: 'one' },
          { url: 'https://example.invalid/two.png', caption: 'two' },
        ],
        index: 0,
      },
      attachTo: document.body,
      global: { plugins: [createAppI18n(['en'])] },
    })

    const prevBtn = () => document.body.querySelector('button.nav.prev') as HTMLButtonElement
    const nextBtn = () => document.body.querySelector('button.nav.next') as HTMLButtonElement
    const counter = () => document.body.querySelector('.counter')?.textContent?.trim()

    expect(counter()).toBe('1 / 2')
    expect(prevBtn().disabled).toBe(true)
    expect(nextBtn().disabled).toBe(false)

    // Zoom in first, then switch to next image -> zoom resets to 100%.
    ;(document.body.querySelector('button.zoom-in') as HTMLButtonElement).click()
    await nextTick()
    expect(document.body.querySelector('.zoom-reset')?.textContent?.trim()).toBe('125%')

    nextBtn().click()
    await nextTick()
    expect(counter()).toBe('2 / 2')
    expect(prevBtn().disabled).toBe(false)
    expect(nextBtn().disabled).toBe(true)
    expect(document.body.querySelector('.caption')?.textContent?.trim()).toBe('two')
    expect(document.body.querySelector('.zoom-reset')?.textContent?.trim()).toBe('100%')

    // ArrowLeft navigates back.
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true }))
    await nextTick()
    expect(counter()).toBe('1 / 2')
    expect(document.body.querySelector('.caption')?.textContent?.trim()).toBe('one')

    // ArrowRight navigates forward.
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }))
    await nextTick()
    expect(counter()).toBe('2 / 2')

    wrapper.unmount()
  })

  it('supports zoom controls, keyboard shortcuts, double-click zoom, pan drag, and Escape/scrim close', async () => {
    const wrapper = mount(ImageLightbox, {
      props: { url: 'https://example.invalid/pic.png', caption: 'pic' },
      attachTo: document.body,
      global: { plugins: [createAppI18n(['en'])] },
    })

    const zoomReset = document.body.querySelector('button.zoom-reset') as HTMLButtonElement
    const picture = document.body.querySelector('img.picture') as HTMLElement

    // Double-click toggles 100% <-> 200%.
    picture.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
    await nextTick()
    expect(zoomReset.textContent?.trim()).toBe('200%')
    expect(picture.classList.contains('zoomed')).toBe(true)

    // Pointer drag pans while zoomed and does not close on scrim click right after.
    picture.dispatchEvent(
      new PointerEvent('pointerdown', { button: 0, pointerId: 1, clientX: 100, clientY: 100 }),
    )
    await nextTick()
    expect(picture.classList.contains('dragging')).toBe(true)
    picture.dispatchEvent(new PointerEvent('pointermove', { pointerId: 1, clientX: 140, clientY: 125 }))
    await nextTick()
    expect(picture.style.transform).toContain('translate3d(40px, 25px, 0) scale(2)')
    picture.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1, clientX: 140, clientY: 125 }))
    await nextTick()
    expect(picture.classList.contains('dragging')).toBe(false)
    // A click immediately following a drag does not close the lightbox.
    ;(document.body.querySelector('.scrim') as HTMLElement).click()
    expect(wrapper.emitted('close')).toBeUndefined()

    // Keyboard '+' zooms in, '-' zooms out, '0' resets to 100%.
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '+', bubbles: true, cancelable: true }))
    await nextTick()
    expect(zoomReset.textContent?.trim()).toBe('225%')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '-', bubbles: true, cancelable: true }))
    await nextTick()
    expect(zoomReset.textContent?.trim()).toBe('200%')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '0', bubbles: true, cancelable: true }))
    await nextTick()
    expect(zoomReset.textContent?.trim()).toBe('100%')
    expect(picture.style.transform).toContain('translate3d(0px, 0px, 0) scale(1)')
    // Modifier combos (browser zoom, Alt+ArrowLeft) are not swallowed; Escape still closes.
    const ctrlZero = new KeyboardEvent('keydown', {
      key: '0',
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    })
    window.dispatchEvent(ctrlZero)
    expect(ctrlZero.defaultPrevented).toBe(false)
    const altLeft = new KeyboardEvent('keydown', {
      key: 'ArrowLeft',
      altKey: true,
      bubbles: true,
      cancelable: true,
    })
    window.dispatchEvent(altLeft)
    expect(altLeft.defaultPrevented).toBe(false)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    expect(wrapper.emitted('close')).toHaveLength(1)
    await new Promise((resolve) => setTimeout(resolve, 0))
    ;(document.body.querySelector('.scrim') as HTMLElement).click()
    expect(wrapper.emitted('close')).toHaveLength(2)

    wrapper.unmount()
  })

  it('navigates across images from multiple blocks inside ChatView', async () => {
    const originalResizeObserver = globalThis.ResizeObserver
    globalThis.ResizeObserver = class {
      observe() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver
    try {
      const blocks: Block[] = [
        {
          kind: 'user',
          key: 'm1:0',
          messageId: 'm1',
          streaming: false,
          stale: false,
          text: 'first',
          source: 'human',
          senderName: undefined,
          opensTurn: false,
          images: [{ id: 'img_1', name: 'a.png', mime: 'image/png', size: 120 }],
        },
        {
          kind: 'user',
          key: 'm2:0',
          messageId: 'm2',
          streaming: false,
          stale: false,
          text: 'second',
          source: 'human',
          senderName: undefined,
          opensTurn: true,
          images: [{ id: 'img_2', name: 'b.png', mime: 'image/png', size: 240 }],
        },
      ]
      const session = shallowReactive({
        blocks,
        activity: 'idle',
        busy: false,
        generation: 1,
        moreBefore: false,
        loadingOlder: false,
        olderError: '',
        loadOlder: vi.fn(),
        imageUrl: vi.fn((id: string) => Promise.resolve(`data:image/png;base64,${id}`)),
      })
      const wrapper = mount(ChatView, {
        props: { session: session as unknown as ChatSession, following: true },
        attachTo: document.body,
        global: {
          plugins: [createPinia(), createAppI18n(['en'])],
          provide: { [chatSessionKey as symbol]: session as unknown as ChatSession },
        },
      })
      await nextTick()
      await nextTick()
      const thumbs = wrapper.findAll('button.thumb')
      expect(thumbs).toHaveLength(2)
      await thumbs[0]!.trigger('click')
      await nextTick()

      expect(document.body.querySelector('.scrim .counter')?.textContent?.trim()).toBe('1 / 2')
      expect(document.body.querySelector('.scrim .caption')?.textContent?.trim()).toContain('a.png')

      ;(document.body.querySelector('.scrim button.nav.next') as HTMLButtonElement).click()
      await nextTick()
      expect(document.body.querySelector('.scrim .counter')?.textContent?.trim()).toBe('2 / 2')
      expect(document.body.querySelector('.scrim .caption')?.textContent?.trim()).toContain('b.png')
      wrapper.unmount()
    } finally {
      globalThis.ResizeObserver = originalResizeObserver
    }
  })

  it('smoothly zooms on ctrl+wheel trackpad pinch without jumping by 1.15x', async () => {
    const wrapper = mount(ImageLightbox, {
      props: { url: 'https://example.invalid/pic.png', caption: 'pic' },
      attachTo: document.body,
      global: { plugins: [createAppI18n(['en'])] },
    })

    const picture = document.body.querySelector('.picture') as HTMLElement
    const zoomReset = () => document.body.querySelector('.zoom-reset')?.textContent?.trim()

    const makeCtrlWheel = (deltaY: number): WheelEvent => {
      const event = new WheelEvent('wheel', { deltaY, bubbles: true, cancelable: true })
      Object.defineProperty(event, 'ctrlKey', { value: true })
      return event
    }

    const pinchIn = makeCtrlWheel(-4)
    picture.dispatchEvent(pinchIn)
    expect(pinchIn.defaultPrevented).toBe(true)
    await nextTick()
    expect(zoomReset()).toBe('104%')

    picture.dispatchEvent(makeCtrlWheel(-6))
    await nextTick()
    expect(zoomReset()).toBe('110%')

    picture.dispatchEvent(makeCtrlWheel(10))
    await nextTick()
    expect(zoomReset()).toBe('100%')

    wrapper.unmount()
  })

  it('supports 2-finger touchscreen pinch-to-zoom and cleans up on release without closing', async () => {
    const wrapper = mount(ImageLightbox, {
      props: { url: 'https://example.invalid/pic.png', caption: 'pic' },
      attachTo: document.body,
      global: { plugins: [createAppI18n(['en'])] },
    })

    const scrim = document.body.querySelector('.scrim') as HTMLElement
    const picture = document.body.querySelector('img.picture') as HTMLElement
    const zoomReset = () => document.body.querySelector('.zoom-reset')?.textContent?.trim()

    // Two touch fingers down on the image (bubbles to .stage even at 1x).
    picture.dispatchEvent(
      new PointerEvent('pointerdown', {
        pointerId: 11,
        pointerType: 'touch',
        clientX: 100,
        clientY: 100,
        bubbles: true,
      }),
    )
    picture.dispatchEvent(
      new PointerEvent('pointerdown', {
        pointerId: 12,
        pointerType: 'touch',
        clientX: 200,
        clientY: 100,
        bubbles: true,
      }),
    )
    await nextTick()
    expect(picture.classList.contains('pinching')).toBe(true)

    // Spread fingers to 2x distance (100px -> 200px) and shift midpoint (+20, +10).
    picture.dispatchEvent(
      new PointerEvent('pointermove', {
        pointerId: 11,
        pointerType: 'touch',
        clientX: 70,
        clientY: 110,
        bubbles: true,
      }),
    )
    picture.dispatchEvent(
      new PointerEvent('pointermove', {
        pointerId: 12,
        pointerType: 'touch',
        clientX: 270,
        clientY: 110,
        bubbles: true,
      }),
    )
    await nextTick()
    expect(zoomReset()).toBe('200%')
    expect(picture.classList.contains('zoomed')).toBe(true)
    expect(picture.style.transform).toContain('translate3d(20px, 10px, 0) scale(2)')

    // Release fingers; immediate click on scrim must not close the lightbox.
    picture.dispatchEvent(
      new PointerEvent('pointerup', {
        pointerId: 12,
        pointerType: 'touch',
        clientX: 270,
        clientY: 110,
        bubbles: true,
      }),
    )
    picture.dispatchEvent(
      new PointerEvent('pointerup', {
        pointerId: 11,
        pointerType: 'touch',
        clientX: 70,
        clientY: 110,
        bubbles: true,
      }),
    )
    await nextTick()
    expect(picture.classList.contains('pinching')).toBe(false)

    scrim.click()
    expect(wrapper.emitted('close')).toBeUndefined()

    await new Promise((resolve) => setTimeout(resolve, 0))
    scrim.click()
    expect(wrapper.emitted('close')).toHaveLength(1)

    wrapper.unmount()
  })

  it('cleans up touch pointers on lostpointercapture and clamps ctrl+wheel mouse zoom', async () => {
    const wrapper = mount(ImageLightbox, {
      props: { url: 'https://example.invalid/pic.png', caption: 'pic' },
      attachTo: document.body,
      global: { plugins: [createAppI18n(['en'])] },
    })

    const stage = document.body.querySelector('.stage') as HTMLElement
    const picture = document.body.querySelector('img.picture') as HTMLElement
    const zoomReset = () => document.body.querySelector('.zoom-reset')?.textContent?.trim()

    picture.dispatchEvent(
      new PointerEvent('pointerdown', {
        pointerId: 21,
        pointerType: 'touch',
        clientX: 100,
        clientY: 100,
        bubbles: true,
      }),
    )
    picture.dispatchEvent(
      new PointerEvent('pointerdown', {
        pointerId: 22,
        pointerType: 'touch',
        clientX: 200,
        clientY: 100,
        bubbles: true,
      }),
    )
    await nextTick()
    expect(picture.classList.contains('pinching')).toBe(true)

    stage.dispatchEvent(new PointerEvent('lostpointercapture', { pointerId: 22, pointerType: 'touch' }))
    stage.dispatchEvent(new PointerEvent('lostpointercapture', { pointerId: 21, pointerType: 'touch' }))
    await nextTick()
    expect(picture.classList.contains('pinching')).toBe(false)

    // A single subsequent touch does not re-enter pinching (no lingering ghost pointer).
    picture.dispatchEvent(
      new PointerEvent('pointerdown', {
        pointerId: 23,
        pointerType: 'touch',
        clientX: 120,
        clientY: 120,
        bubbles: true,
      }),
    )
    await nextTick()
    expect(picture.classList.contains('pinching')).toBe(false)
    picture.dispatchEvent(
      new PointerEvent('pointerup', { pointerId: 23, pointerType: 'touch', bubbles: true }),
    )

    // Mouse Ctrl+wheel (deltaY = -120) is clamped instead of jumping ~3.32x.
    const ctrlMouseWheel = new WheelEvent('wheel', {
      deltaY: -120,
      bubbles: true,
      cancelable: true,
    })
    Object.defineProperty(ctrlMouseWheel, 'ctrlKey', { value: true })
    picture.dispatchEvent(ctrlMouseWheel)
    expect(ctrlMouseWheel.defaultPrevented).toBe(true)
    await nextTick()
    expect(zoomReset()).toBe('149%')

    wrapper.unmount()
  })
})
