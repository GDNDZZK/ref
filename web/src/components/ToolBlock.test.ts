import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { ref } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import editWarningsPart from '../../../crates/proto/fixtures/part/tool_done_edit_warnings.json'
import { chatScrollKey } from '../composables/chatScroll.ts'
import { chatSessionKey } from '../composables/chatSessionContext.ts'
import { createAppI18n } from '../i18n/index.ts'
import type { ToolBlock as ToolBlockModel } from '../model/blocks.ts'
import type { ChatSession } from '../model/chatSession.ts'
import type { JsonValue, Part } from '../proto/generated'
import ToolBlock from './ToolBlock.vue'

type ToolPart = Extract<Part, { type: 'tool' }>

function blockFromPart(part: ToolPart): ToolBlockModel {
  return {
    kind: 'tool',
    key: part.call_id,
    messageId: 'm1',
    stale: false,
    streaming: false,
    callId: part.call_id,
    name: part.name,
    purpose: part.purpose,
    state: part.state,
    input: part.input as JsonValue,
    rawInput: part.raw_input,
    output: part.output,
    meta: (part.meta ?? null) as JsonValue | null,
    time: part.time,
    timeEnd: part.time_end,
    runAt: part.run_at ?? null,
    permission: undefined,
    question: undefined,
    images: [],
  }
}

function mountToolBlock(block: ToolBlockModel, detailsExpanded = false) {
  const session = {
    detailsExpanded,
    permissions: [],
    isSendingAnswer: () => false,
    answerPermission: vi.fn().mockResolvedValue(undefined),
    answerQuestion: vi.fn().mockResolvedValue(undefined),
    stop: vi.fn().mockResolvedValue(undefined),
    questionForm: vi.fn(),
  } as unknown as ChatSession

  return mount(ToolBlock, {
    props: { block },
    global: {
      plugins: [createPinia(), createAppI18n(['en'])],
      provide: {
        [chatSessionKey as symbol]: session,
        [chatScrollKey as symbol]: {
          scroller: ref(null),
          hoveredMessageId: ref(''),
          holdHeader: vi.fn(),
          releaseHeader: vi.fn(),
        },
      },
    },
  })
}

describe('ToolBlock', () => {
  it('renders meta.warnings below the diff from tool_done_edit_warnings.json', () => {
    const part = editWarningsPart as unknown as ToolPart
    const wrapper = mountToolBlock(blockFromPart(part), true)

    const diffEl = wrapper.get('.rows').element
    const warningEl = wrapper.get('.warning').element
    expect(warningEl.textContent).toBe('Recovered: file changed since read; line numbers remapped.')
    expect(diffEl.compareDocumentPosition(warningEl) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })
})
