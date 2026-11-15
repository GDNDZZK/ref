import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, shallowReactive } from 'vue'
import { useChatScroll } from '../composables/chatScroll.ts'
import type { ChatScroll } from '../composables/chatScroll.ts'
import type { Activity, ChatSession } from '../model/chatSession.ts'
import type { Block } from '../model/blocks.ts'
import { motion } from '../theme/generated/motion.ts'
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

function heldPx(footer: HTMLElement): number {
  const match = footer.style.height.match(/([\d.]+)px\)$/)
  return match ? Number(match[1]) : 0
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
  // Simulate browser clamping: scrollHeight is always at least clientHeight.
  Object.defineProperty(scroller, 'scrollHeight', {
    configurable: true,
    get: () => Math.max(clientHeight, rowsHeight + PAGE_PADDING + heldPx(footer)),
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

describe('ChatView hold and scroll geometry', () => {
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

  it.each<Activity>(['waiting_permission', 'waiting_question', 'retrying', 'idle'])(
    'releases heldSpace when transitioning from active generation to %s',
    async (nextActivity) => {
      const { wrapper, session, footer, setRowsHeight } = setupChatView({
        activity: 'thinking',
        clientHeight: 800,
        rowsHeight: 1000,
      })
      setRowsHeight(1000)
      setRowsHeight(820)
      expect(heldPx(footer)).toBe(180)

      session.activity = nextActivity
      session.busy = nextActivity !== 'idle'
      await nextTick()

      // Release tween runs over motion.slow ms via requestAnimationFrame.
      await vi.advanceTimersByTimeAsync(motion.slow + 50)
      expect(heldPx(footer)).toBe(0)

      // Shrinking while paused/waiting must not re-hold space.
      setRowsHeight(700)
      expect(heldPx(footer)).toBe(0)

      wrapper.unmount()
    },
  )

  it('holds during thinking, running, and compacting', async () => {
    const { wrapper, session, footer, setRowsHeight } = setupChatView({
      activity: 'thinking',
      clientHeight: 800,
      rowsHeight: 1000,
    })

    setRowsHeight(1000)
    setRowsHeight(900)
    expect(heldPx(footer)).toBe(100)

    session.activity = 'running'
    await nextTick()
    setRowsHeight(850)
    expect(heldPx(footer)).toBe(150)

    session.activity = 'compacting'
    await nextTick()
    setRowsHeight(800)
    expect(heldPx(footer)).toBe(200)

    wrapper.unmount()
  })

  it('does not inflate heldSpace when scroller.scrollHeight is clamped to clientHeight', async () => {
    const { wrapper, session, footer, setRowsHeight } = setupChatView({
      activity: 'idle',
      clientHeight: 800,
      rowsHeight: 200,
    })

    // Start active turn on a short conversation (scrollHeight clamped at 800).
    session.activity = 'thinking'
    session.busy = true
    await nextTick()
    expect(heldPx(footer)).toBe(0)

    // Grows while still shorter than clientHeight, then shrinks slightly.
    setRowsHeight(350)
    expect(heldPx(footer)).toBe(0)

    setRowsHeight(250)
    expect(heldPx(footer)).toBe(100) // not clientHeight - 1 (799)

    // Shrinking from above clientHeight down below clientHeight holds only the actual delta.
    setRowsHeight(900)
    expect(heldPx(footer)).toBe(0)

    setRowsHeight(750)
    expect(heldPx(footer)).toBe(150) // 900 - 750, not 200
    session.activity = 'idle'
    session.busy = false
    await nextTick()
    await vi.advanceTimersByTimeAsync(motion.slow + 50)
    expect(heldPx(footer)).toBe(0)

    wrapper.unmount()
  })

  it('resets highWater on jumpToEnd across a mid-turn generation reload', async () => {
    const { wrapper, session, footer, setRowsHeight } = setupChatView({
      activity: 'thinking',
      clientHeight: 800,
      rowsHeight: 2000,
    })

    setRowsHeight(2000)
    setRowsHeight(1700)
    expect(heldPx(footer)).toBe(200)
    // Mid-turn reload (e.g. resync) loads shorter history while following is still true.
    session.generation += 1
    session.blocks = [makeBlock('b2')]
    setRowsHeight(400, false)
    await nextTick()
    await nextTick()

    expect(heldPx(footer)).toBe(0)

    // Subsequent resize must not resurrect highWater = 2000 from before the reload.
    setRowsHeight(400)
    expect(heldPx(footer)).toBe(0)

    setRowsHeight(380)
    expect(heldPx(footer)).toBe(20)

    wrapper.unmount()
  })

  it('cancels an in-flight release tween on jumpToEnd', async () => {
    const { wrapper, session, footer, setRowsHeight } = setupChatView({
      activity: 'thinking',
      clientHeight: 600,
      rowsHeight: 1000,
    })

    setRowsHeight(1000)
    setRowsHeight(700)
    expect(heldPx(footer)).toBe(150)
    // Begin releasing heldSpace.
    session.activity = 'idle'
    session.busy = false
    await nextTick()
    await vi.advanceTimersByTimeAsync(100)
    expect(heldPx(footer)).toBeGreaterThan(0)

    // Reload history mid-release: jumpToEnd must cancel the tween so later frames do not restore heldSpace.
    session.generation += 1
    session.blocks = [makeBlock('b3')]
    setRowsHeight(200, false)
    await nextTick()
    await nextTick()
    expect(heldPx(footer)).toBe(0)

    await vi.advanceTimersByTimeAsync(motion.slow)
    expect(heldPx(footer)).toBe(0)

    wrapper.unmount()
  })

  it('re-engages following with naturalEnd + heldSpace without scrollHeight pollution', async () => {
    const { wrapper, footer, setRowsHeight } = setupChatView({
      activity: 'thinking',
      clientHeight: 800,
      rowsHeight: 200,
    })

    setRowsHeight(300)
    setRowsHeight(220)
    expect(heldPx(footer)).toBe(80)

    await wrapper.setProps({ following: false })
    await wrapper.setProps({ following: true })
    expect(heldPx(footer)).toBe(80)

    // Growing by 50 eats into the 80px held space cleanly rather than jumping to clientHeight.
    setRowsHeight(270)
    expect(heldPx(footer)).toBe(30)

    wrapper.unmount()
  })

  it('caps highWater to 0.25 of clientHeight and handles empty conversations without inflating', async () => {
    const { wrapper, session, footer, setRowsHeight, setClientHeight } = setupChatView({
      activity: 'thinking',
      clientHeight: 800,
      rowsHeight: 0,
    })

    setRowsHeight(0)
    expect(heldPx(footer)).toBe(0)

    setRowsHeight(1500)
    setRowsHeight(700) // shrinks by 800, capped to round(800 * 0.25) = 200
    expect(heldPx(footer)).toBe(200)

    // Viewport shrinks to 500 -> cap becomes round(500 * 0.25) = 125; subsequent growth eats into 125.
    setClientHeight(500)
    expect(heldPx(footer)).toBe(125)
    setRowsHeight(750)
    expect(heldPx(footer)).toBe(75)

    session.activity = 'idle'
    session.busy = false
    await nextTick()
    await vi.advanceTimersByTimeAsync(motion.slow + 50)
    expect(heldPx(footer)).toBe(0)

    wrapper.unmount()
  })

  it('restHold drops heldSpace, prevents fold shrinkage during motion.slow, and resumes after', async () => {
    const { wrapper, footer, chatScroll, setRowsHeight } = setupChatView({
      activity: 'thinking',
      clientHeight: 800,
      rowsHeight: 1000,
    })

    setRowsHeight(1000)
    setRowsHeight(900)
    expect(heldPx(footer)).toBe(100)

    chatScroll.restHold()
    expect(heldPx(footer)).toBe(0)

    // Folding during holdResting follows content down without growing heldSpace.
    setRowsHeight(500)
    expect(heldPx(footer)).toBe(0)

    await vi.advanceTimersByTimeAsync(motion.slow - 1)
    setRowsHeight(450)
    expect(heldPx(footer)).toBe(0)

    // Once motion.slow elapses, normal holding resumes from the post-fold height.
    await vi.advanceTimersByTimeAsync(1)
    setRowsHeight(520)
    expect(heldPx(footer)).toBe(0)
    setRowsHeight(470)
    expect(heldPx(footer)).toBe(50)

    wrapper.unmount()
  })

  it('calls restHold when a leaving row is added while holding', async () => {
    const { wrapper, session, footer, setRowsHeight } = setupChatView({
      activity: 'thinking',
      clientHeight: 800,
      rowsHeight: 1000,
    })

    setRowsHeight(1000)
    session.blocks = []
    await nextTick()

    setRowsHeight(600)
    expect(heldPx(footer)).toBe(0)

    await vi.advanceTimersByTimeAsync(motion.slow)
    setRowsHeight(680)
    setRowsHeight(630)
    expect(heldPx(footer)).toBe(50)

    wrapper.unmount()
  })

  it('tracks knownBlocks across multiple updates so added-then-removed blocks leave once', async () => {
    const { wrapper, session, footer, setRowsHeight } = setupChatView({
      activity: 'thinking',
      clientHeight: 800,
      rowsHeight: 1000,
    })

    setRowsHeight(1000)
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

    setRowsHeight(600)
    expect(heldPx(footer)).toBe(0) // restHold fired when b2 started leaving

    await wrapper.get('.block-stub[data-key="b2"]').trigger('click')
    await nextTick()
    expect(wrapper.findAll('.block-stub').map((el) => el.attributes('data-key'))).toEqual(['b1'])

    await vi.advanceTimersByTimeAsync(motion.slow + 50)
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
