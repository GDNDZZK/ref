import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { nextTick, ref, shallowReactive } from 'vue'
import { chatScrollKey } from '../composables/chatScroll.ts'
import { chatSessionKey } from '../composables/chatSessionContext.ts'
import { createAppI18n } from '../i18n/index.ts'
import type { ReasoningBlock as ReasoningBlockModel } from '../model/blocks.ts'
import type { ChatSession } from '../model/chatSession.ts'
import ReasoningBlock from './ReasoningBlock.vue'

function makeBlock(streaming: boolean): ReasoningBlockModel {
  return {
    kind: 'reasoning',
    key: 'r1',
    messageId: 'm1',
    stale: false,
    streaming,
    text: 'thinking step',
  }
}

function mountReasoningBlock(options: { streaming?: boolean; thinkingExpanded?: boolean } = {}) {
  const restHold = vi.fn()
  const session = shallowReactive({
    thinkingExpanded: options.thinkingExpanded ?? false,
  })
  const wrapper = mount(ReasoningBlock, {
    props: { block: makeBlock(options.streaming ?? false) },
    global: {
      plugins: [createAppI18n(['en'])],
      provide: {
        [chatSessionKey as symbol]: session as unknown as ChatSession,
        [chatScrollKey as symbol]: {
          scroller: ref(null),
          hoveredMessageId: ref(''),
          holdHeader: vi.fn(),
          releaseHeader: vi.fn(),
          restHold,
        },
      },
    },
  })
  return { wrapper, restHold, session }
}

describe('ReasoningBlock', () => {
  it('calls chatScroll.restHold when streaming ends and auto-collapses', async () => {
    const { wrapper, restHold } = mountReasoningBlock({ streaming: true })
    expect(restHold).not.toHaveBeenCalled()

    await wrapper.setProps({ block: makeBlock(false) })
    expect(restHold).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })

  it('calls chatScroll.restHold when user clicks to collapse', async () => {
    const { wrapper, restHold } = mountReasoningBlock({ streaming: true })
    await wrapper.get('button.header').trigger('click')
    expect(restHold).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })

  it('calls chatScroll.restHold when session.thinkingExpanded closes', async () => {
    const { wrapper, restHold, session } = mountReasoningBlock({ streaming: false, thinkingExpanded: true })
    expect(restHold).not.toHaveBeenCalled()

    session.thinkingExpanded = false
    await nextTick()
    expect(restHold).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })
})
