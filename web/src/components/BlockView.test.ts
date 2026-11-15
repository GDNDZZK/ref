import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'
import { chatScrollKey } from '../composables/chatScroll.ts'
import type { Block } from '../model/blocks.ts'
import BlockView from './BlockView.vue'

function makeBlock(): Block {
  return {
    kind: 'markdown',
    key: 'b1',
    messageId: 'm1',
    stale: false,
    streaming: false,
    text: 'hello',
  }
}

function mountBlockView(leaving = false) {
  return mount(BlockView, {
    props: { block: makeBlock(), fresh: false, leaving },
    global: {
      provide: {
        [chatScrollKey as symbol]: {
          scroller: ref(null),
          hoveredMessageId: ref(''),
          holdHeader: vi.fn(),
          releaseHeader: vi.fn(),
          restHold: vi.fn(),
        },
      },
      stubs: { MarkdownBlock: true },
    },
  })
}

function setDocumentHidden(hidden: boolean): void {
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => hidden })
}

describe('BlockView leaving', () => {
  afterEach(() => {
    setDocumentHidden(false)
    vi.restoreAllMocks()
  })

  it('emits gone immediately without animating when document.hidden is true', async () => {
    setDocumentHidden(true)
    const wrapper = mountBlockView(false)
    const animateSpy = vi.spyOn(wrapper.element as HTMLElement, 'animate')

    await wrapper.setProps({ leaving: true })
    await nextTick()

    expect(animateSpy).not.toHaveBeenCalled()
    expect(wrapper.emitted('gone')).toHaveLength(1)
    wrapper.unmount()
  })

  it('waits for fade and close animations before emitting gone when visible', async () => {
    setDocumentHidden(false)
    const wrapper = mountBlockView(false)

    let resolveFade!: () => void
    let resolveClose!: () => void
    const animateSpy = vi
      .spyOn(wrapper.element as HTMLElement, 'animate')
      .mockReturnValueOnce({
        finished: new Promise<Animation>((resolve) => {
          resolveFade = () => resolve({} as Animation)
        }),
      } as unknown as Animation)
      .mockReturnValueOnce({
        finished: new Promise<Animation>((resolve) => {
          resolveClose = () => resolve({} as Animation)
        }),
      } as unknown as Animation)

    await wrapper.setProps({ leaving: true })
    await nextTick()

    expect(animateSpy).toHaveBeenCalledTimes(1)
    expect(wrapper.emitted('gone')).toBeUndefined()

    resolveFade()
    await nextTick()
    await nextTick()
    expect(animateSpy).toHaveBeenCalledTimes(2)
    expect(wrapper.emitted('gone')).toBeUndefined()

    resolveClose()
    await nextTick()
    await nextTick()
    expect(wrapper.emitted('gone')).toHaveLength(1)
    wrapper.unmount()
  })
})
