import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, shallowReactive } from 'vue'
import { useChatScroll } from '../composables/chatScroll.ts'
import type { ChatScroll } from '../composables/chatScroll.ts'
import type { Activity, ChatSession } from '../model/chatSession.ts'
import type { Block } from '../model/blocks.ts'
import ChatView from './ChatView.vue'

interface ResizeEntryStub {
  target: Element
}

class MockResizeObserver {
  static instances: MockResizeObserver[] = []
  private readonly callback: (entries: ResizeEntryStub[]) => void
  readonly targets = new Set<Element>()

  constructor(callback: (entries: ResizeEntryStub[]) => void) {
    this.callback = callback
    MockResizeObserver.instances.push(this)
  }

  observe(target: Element): void {
    this.targets.add(target)
  }

  disconnect(): void {
    this.targets.clear()
  }

  trigger(targets: Element[]): void {
    const observed = targets.filter((target) => this.targets.has(target))
    if (observed.length > 0) this.callback(observed.map((target) => ({ target })))
  }
}

const PAGE_PADDING = 24

function makeBlock(key: string): Block {
  return {
    kind: 'markdown',
    key,
    messageId: `m-${key}`,
    stale: false,
    streaming: false,
    text: `text ${key}`,
  }
}

function setupChatView(options: { activity?: Activity; clientHeight?: number; rowsHeight?: number } = {}) {
  let rowsHeight = options.rowsHeight ?? 200
  let clientHeight = options.clientHeight ?? 800
  const session = shallowReactive({
    blocks: [makeBlock('b1')] as readonly Block[],
    activity: (options.activity ?? 'idle') as Activity,
    busy: (options.activity ?? 'idle') !== 'idle',
    generation: 1,
    moreBefore: false,
    loadingOlder: false,
    olderError: '',
    loadOlder: vi.fn(),
  })

  let chatScroll: ChatScroll | undefined
  const wrapper = mount(ChatView, {
    props: { session: session as unknown as ChatSession, following: true },
    global: {
      stubs: {
        BlockView: {
          props: ['block', 'fresh', 'leaving'],
          emits: ['gone'],
          setup() {
            chatScroll = useChatScroll()
            return {}
          },
          template:
            '<div class="block-stub" :data-key="block.key" :data-leaving="String(leaving)" @click="$emit(\'gone\')" />',
        },
        OlderMessages: true,
      },
    },
  })
  const scroller = wrapper.get('.scroller').element as HTMLElement
  const rows = wrapper.get('.rows').element as HTMLElement
  const footer = wrapper.get('.footer').element as HTMLElement

  Object.defineProperty(rows, 'offsetHeight', { configurable: true, get: () => rowsHeight })
  Object.defineProperty(scroller, 'clientHeight', { configurable: true, get: () => clientHeight })
  Object.defineProperty(scroller, 'scrollHeight', {
    configurable: true,
    get: () => Math.max(clientHeight, rowsHeight + PAGE_PADDING),
  })
  const resizeObserver = MockResizeObserver.instances.find((observer) => observer.targets.has(rows))!

  return {
    wrapper,
    session,
    scroller,
    rows,
    footer,
    chatScroll: chatScroll!,
    setRowsHeight(height: number, notify = true) {
      rowsHeight = height
      if (notify) resizeObserver.trigger([rows])
    },
    setClientHeight(height: number, notify = true) {
      clientHeight = height
      if (notify) resizeObserver.trigger([scroller])
    },
  }
}

describe('ChatView scroll geometry', () => {
  const originalResizeObserver = globalThis.ResizeObserver

  beforeEach(() => {
    vi.useFakeTimers()
    MockResizeObserver.instances = []
    globalThis.ResizeObserver = MockResizeObserver as unknown as typeof ResizeObserver
  })

  afterEach(() => {
    vi.useRealTimers()
    globalThis.ResizeObserver = originalResizeObserver
  })

  it.each<Activity>(['thinking', 'running', 'compacting', 'waiting_permission', 'waiting_question', 'idle'])(
    'never inflates footer height beyond pagePadding when rows shrink during %s',
    (activity) => {
      const { wrapper, footer, setRowsHeight } = setupChatView({
        activity,
        clientHeight: 800,
        rowsHeight: 1200,
      })
      setRowsHeight(1200)
      setRowsHeight(700)
      expect(footer.style.height).toBe('')
      wrapper.unmount()
    },
  )

  it('jumps to scrollHeight and re-engages following on generation reload', async () => {
    const { wrapper, session, scroller, setRowsHeight } = setupChatView({
      activity: 'idle',
      clientHeight: 600,
      rowsHeight: 1400,
    })
    await wrapper.setProps({ following: false })
    scroller.scrollTop = 120

    session.generation += 1
    session.blocks = [makeBlock('b2')]
    setRowsHeight(1000, false)
    await nextTick()
    await nextTick()

    expect(scroller.scrollTop).toBe(1000 + PAGE_PADDING)
    expect(wrapper.emitted('update:following')?.at(-1)).toEqual([true])
    wrapper.unmount()
  })

  it('holds sticky header in view while its block shrinks and stops after releaseHeader', () => {
    const { wrapper, scroller, chatScroll } = setupChatView({ clientHeight: 500, rowsHeight: 1600 })
    scroller.scrollTop = 400
    vi.spyOn(scroller, 'getBoundingClientRect').mockReturnValue({ top: 0 } as DOMRect)

    const blockEl = document.createElement('div')
    let blockHeight = 500
    Object.defineProperty(blockEl, 'offsetHeight', { configurable: true, get: () => blockHeight })
    // Block starts at document y = 200 (viewport top = 200 - scrollTop).
    vi.spyOn(blockEl, 'getBoundingClientRect').mockImplementation(
      () => ({ top: 200 - scroller.scrollTop }) as DOMRect,
    )

    chatScroll.holdHeader(blockEl, 38)
    const holdObserver = MockResizeObserver.instances.find((observer) => observer.targets.has(blockEl))!

    // Shrink block from 500 to 150 -> pinLimit = 200 + 150 - 38 = 312 < scrollTop 400.
    blockHeight = 150
    holdObserver.trigger([blockEl])
    expect(scroller.scrollTop).toBe(312)

    // Once released, further shrinkage does not clamp scrollTop.
    chatScroll.releaseHeader()
    blockHeight = 80
    holdObserver.trigger([blockEl])
    expect(scroller.scrollTop).toBe(312)
    wrapper.unmount()
  })

  it('tracks knownBlocks across multiple updates so added-then-removed blocks leave once', async () => {
    const { wrapper, session } = setupChatView({
      activity: 'thinking',
      clientHeight: 800,
      rowsHeight: 1000,
    })

    session.blocks = [makeBlock('b1'), makeBlock('b2')]
    await nextTick()
    session.blocks = [makeBlock('b1')]
    await nextTick()
    expect(
      wrapper.findAll('.block-stub').map((el) => [el.attributes('data-key'), el.attributes('data-leaving')]),
    ).toEqual([
      ['b1', 'false'],
      ['b2', 'true'],
    ])

    await wrapper.get('.block-stub[data-key="b2"]').trigger('click')
    await nextTick()
    expect(wrapper.findAll('.block-stub').map((el) => el.attributes('data-key'))).toEqual(['b1'])

    session.blocks = [makeBlock('b1'), makeBlock('b3')]
    await nextTick()
    expect(
      wrapper.findAll('.block-stub').map((el) => [el.attributes('data-key'), el.attributes('data-leaving')]),
    ).toEqual([
      ['b1', 'false'],
      ['b3', 'false'],
    ])
    wrapper.unmount()
  })
})
