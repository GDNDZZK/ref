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
        },
      },
    },
  })
  return { wrapper, session }
}

describe('ReasoningBlock', () => {
  it('stays expanded while streaming and auto-collapses when streaming ends', async () => {
    const { wrapper } = mountReasoningBlock({ streaming: true })
    expect(wrapper.get('.body').classes()).toContain('open')

    await wrapper.setProps({ block: makeBlock(false) })
    expect(wrapper.get('.body').classes()).not.toContain('open')
    wrapper.unmount()
  })

  it('toggles open state when clicking the header', async () => {
    const { wrapper } = mountReasoningBlock({ streaming: false })
    expect(wrapper.get('.body').classes()).not.toContain('open')

    await wrapper.get('button.header').trigger('click')
    expect(wrapper.get('.body').classes()).toContain('open')
    wrapper.unmount()
  })

  it('follows session.thinkingExpanded when not manually flipped', async () => {
    const { wrapper, session } = mountReasoningBlock({ streaming: false, thinkingExpanded: true })
    expect(wrapper.get('.body').classes()).toContain('open')

    session.thinkingExpanded = false
    await nextTick()
    expect(wrapper.get('.body').classes()).not.toContain('open')
    wrapper.unmount()
  })
})
