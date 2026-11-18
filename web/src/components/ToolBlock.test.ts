import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { ref, shallowReactive } from 'vue'
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
  const session = shallowReactive({
    detailsExpanded,
    permissions: [],
    isSendingAnswer: () => false,
    answerPermission: vi.fn().mockResolvedValue(undefined),
    answerQuestion: vi.fn().mockResolvedValue(undefined),
    stop: vi.fn().mockResolvedValue(undefined),
    questionForm: vi.fn(),
  })

  const wrapper = mount(ToolBlock, {
    props: { block },
    global: {
      plugins: [createPinia(), createAppI18n(['en'])],
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
  return Object.assign(wrapper, { session })
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

  it('renders a compact answered summary on a collapsed question tool card', () => {
    const questionBlock: ToolBlockModel = {
      ...blockFromPart(editWarningsPart as unknown as ToolPart),
      name: 'question',
      purpose: 'Ask user',
      input: { questions: [{ header: 'Loop bound', question: 'How?' }] },
      output: 'Loop bound: Range',
      meta: { answers: [['Range']] },
    }
    const wrapper = mountToolBlock(questionBlock, false)
    expect(wrapper.find('.summary.answered').text()).toBe('Loop boundRange')
  })

  it('shows path, counts, first 8 diff rows and +N more on a collapsed write/edit card', () => {
    const longDiff = [
      '--- a/src/file.ts',
      '+++ b/src/file.ts',
      '@@ -1,6 +1,6 @@',
      ' line 1',
      '-old 2',
      '+new 2',
      ' line 3',
      ' line 4',
      ' line 5',
      ' line 6',
    ].join('\n')
    const editBlock: ToolBlockModel = {
      ...blockFromPart(editWarningsPart as unknown as ToolPart),
      name: 'write',
      input: { path: 'src/file.ts', content: 'hello' },
      output: 'Wrote src/file.ts',
      meta: { path: '/work/src/file.ts', diff: longDiff, added: 3, removed: 1 },
    }
    const wrapper = mountToolBlock(editBlock, false)
    expect(wrapper.get('.diff-preview .diff-file').text()).toBe('/work/src/file.ts')
    expect(wrapper.get('.diff-preview .diff-added').text()).toBe('+3')
    expect(wrapper.get('.diff-preview .diff-removed').text()).toBe('-1')
    expect(wrapper.findAll('.diff-preview .row')).toHaveLength(8)
    expect(wrapper.get('.diff-preview .more-note').text()).toBe('+2 more')
  })

  it('renders raw Input, full diff, and Output always when write/edit is expanded', () => {
    const part = editWarningsPart as unknown as ToolPart
    const wrapper = mountToolBlock(blockFromPart(part), true)
    expect(wrapper.find('.diff-preview').exists()).toBe(false)
    expect(wrapper.text()).toContain('Input')
    expect(wrapper.text()).toContain('src/parser.rs')
    expect(wrapper.text()).toContain('Output')
    expect(wrapper.text()).toContain('Edited src/parser.rs')
  })
})
