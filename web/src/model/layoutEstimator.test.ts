import { describe, expect, it } from 'vitest'
import type { MessageLayoutMeta } from '../proto/generated'
import { estimateMessageHeight, layoutMetaFromMessage } from './layoutEstimator.ts'
import { assistantMessage, reasoningPart, textPart, toolPart, userMessage } from './testMessages.ts'

describe('layoutEstimator', () => {
  it('computes layout meta from messages for text, reasoning, image, and tool parts', () => {
    const msg = assistantMessage('a1', [
      reasoningPart('line1\nline2'),
      textPart('Hello\n```ts\nconst x = 1\nconst y = 2\n```\nDone'),
      { type: 'image', mime: 'image/png', data: '', id: 'img1', size: 100 },
      toolPart('c1', 'done'),
    ])
    const meta = layoutMetaFromMessage(4, msg)
    expect(meta.seq).toBe(4)
    expect(meta.id).toBe('a1')
    expect(meta.role).toBe('assistant')
    expect(meta.parts).toHaveLength(4)
    expect(meta.parts[0]).toMatchObject({ kind: 'reasoning', lines: 2 })
    expect(meta.parts[1]).toMatchObject({ kind: 'text', code_blocks: 1, code_lines: 2 })
    expect(meta.parts[2]).toMatchObject({ kind: 'image' })
    expect(meta.parts[3]).toMatchObject({ kind: 'tool', tool: 'bash' })
  })

  it('recomputes heights synchronously on column width and fold state changes', () => {
    const meta: MessageLayoutMeta = {
      seq: 1,
      id: 'a1',
      role: 'assistant',
      source_kind: 'human',
      has_error: false,
      parts: [
        {
          kind: 'reasoning',
          chars: 240,
          lines: 4,
          code_blocks: 0,
          code_lines: 0,
          tables: 0,
          tool: null,
          tool_lines: 0,
          width: null,
          height: null,
        },
        {
          kind: 'text',
          chars: 300,
          lines: 3,
          code_blocks: 0,
          code_lines: 0,
          tables: 0,
          tool: null,
          tool_lines: 0,
          width: null,
          height: null,
        },
        {
          kind: 'tool',
          chars: 120,
          lines: 12,
          code_blocks: 0,
          code_lines: 0,
          tables: 0,
          tool: 'bash',
          tool_lines: 12,
          width: null,
          height: null,
        },
      ],
    }

    const collapsedWide = estimateMessageHeight(meta, {
      columnWidth: 760,
      thinkingExpanded: false,
      detailsExpanded: false,
      isFirst: false,
    })
    const collapsedNarrow = estimateMessageHeight(meta, {
      columnWidth: 320,
      thinkingExpanded: false,
      detailsExpanded: false,
      isFirst: false,
    })
    const expandedThinking = estimateMessageHeight(meta, {
      columnWidth: 760,
      thinkingExpanded: true,
      detailsExpanded: false,
      isFirst: false,
    })
    const expandedDetails = estimateMessageHeight(meta, {
      columnWidth: 760,
      thinkingExpanded: false,
      detailsExpanded: true,
      isFirst: false,
    })

    expect(collapsedNarrow).toBeGreaterThan(collapsedWide)
    expect(expandedThinking).toBeGreaterThan(collapsedWide)
    expect(expandedDetails).toBeGreaterThan(collapsedWide)
  })

  it('accounts for turn divider on non-first user messages and compact height for compaction', () => {
    const userMeta = layoutMetaFromMessage(1, userMessage('u1', 'hi'))
    const firstH = estimateMessageHeight(userMeta, {
      columnWidth: 760,
      thinkingExpanded: false,
      detailsExpanded: false,
      isFirst: true,
    })
    const laterH = estimateMessageHeight(userMeta, {
      columnWidth: 760,
      thinkingExpanded: false,
      detailsExpanded: false,
      isFirst: false,
    })
    expect(laterH).toBeGreaterThan(firstH)

    const compactionMeta = layoutMetaFromMessage(2, userMessage('c1', 'summary', { kind: 'compaction' }))
    expect(
      estimateMessageHeight(compactionMeta, {
        columnWidth: 760,
        thinkingExpanded: false,
        detailsExpanded: false,
        isFirst: false,
      }),
    ).toBe(30)
  })
})
