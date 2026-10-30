import { describe, expect, it } from 'vitest'
import type { SessionEvent } from '../proto/generated'
import type { Block } from './blocks.ts'
import { Transcript } from './transcript.ts'
import {
  assistantMessage,
  info,
  reasoningPart,
  snapshot,
  textPart,
  toolPart,
  userMessage,
} from './testMessages.ts'

const LATER = 100_000 // a time by which the pacer has released everything

function setup() {
  const errors: string[] = []
  const transcript = new Transcript({ reportError: (message) => errors.push(message) })
  return { transcript, errors }
}

// Message footers are tested on their own; the other tests look at the content.
const content = (blocks: readonly Block[]) => blocks.filter((block) => block.kind !== 'meta')
const kinds = (blocks: readonly Block[]) => content(blocks).map((block) => block.kind)
const texts = (blocks: readonly Block[]) =>
  content(blocks).map((block) => ('text' in block ? block.text : ''))

// What the assistant message a1 shows so far.
function shownOf(transcript: Transcript): string {
  return texts(transcript.blocks.filter((block) => block.messageId === 'a1')).join('')
}

function textDelta(messageId: string, index: number, text: string): SessionEvent {
  return { type: 'delta', message_id: messageId, index, delta: { type: 'text', text } }
}

describe('Transcript: loading', () => {
  it('shows a snapshot at once, one block per part and per code fence', () => {
    const { transcript } = setup()
    transcript.load(
      snapshot([
        userMessage('u1', 'Fix it'),
        assistantMessage(
          'a1',
          [
            reasoningPart('Hmm'),
            textPart('Look:\n\n```rust\nlet x = 1;\n```\nDone.'),
            toolPart('c1', 'done'),
          ],
          { time_end: 1 },
        ),
      ]),
    )
    transcript.flush(0)

    expect(kinds(transcript.blocks)).toEqual(['user', 'reasoning', 'markdown', 'code', 'markdown', 'tool'])
    expect(texts(transcript.blocks)).toEqual(['Fix it', 'Hmm', 'Look:', 'let x = 1;', 'Done.', ''])
    expect(transcript.blocks.map((b) => b.key)).toEqual([
      'u1:user',
      'u1:meta',
      'a1:0',
      'a1:1:0',
      'a1:1:1',
      'a1:1:2',
      'a1:2',
      'a1:meta',
    ])
    expect(transcript.blocks.every((block) => !block.streaming)).toBe(true)
  })

  it('puts a divider before every prompt but the first, and before the first when older rows exist', () => {
    const { transcript } = setup()
    transcript.load(
      snapshot([
        userMessage('u1', 'a'),
        assistantMessage('a1', [textPart('r')], { time_end: 1 }),
        userMessage('u2', 'b'),
      ]),
    )
    transcript.flush(0)
    const dividers = () =>
      transcript.blocks.flatMap((block) => (block.kind === 'user' ? [block.opensTurn] : []))
    expect(dividers()).toEqual([false, true])

    transcript.load(snapshot([userMessage('u1', 'a')], { more_before: true }))
    transcript.flush(0)
    expect(dividers()).toEqual([true])
  })

  it('leaves out dividers for shell output and reminders', () => {
    const { transcript } = setup()
    transcript.load(
      snapshot([
        userMessage('u1', 'a'),
        userMessage('s1', '$ ls', { kind: 'shell' }),
        userMessage('r1', 'remember', { kind: 'reminder' }),
        userMessage('p1', 'hi', { kind: 'peer', from_id: 'x', from_name: 'other', msg_id: 'm' }),
      ]),
    )
    transcript.flush(0)
    const users = transcript.blocks.flatMap((block) => (block.kind === 'user' ? [block] : []))
    expect(users.map((b) => b.opensTurn)).toEqual([false, false, false, true])
    expect(users[3]?.senderName).toBe('other')
  })

  it('reset replaces the history', () => {
    const { transcript } = setup()
    transcript.load(snapshot([userMessage('u1', 'old')]))
    transcript.apply({ type: 'reset', messages: [userMessage('u2', 'new')], more_before: false }, 0)
    transcript.flush(0)
    expect(texts(transcript.blocks)).toEqual(['new'])
  })

  it('removes a message', () => {
    const { transcript } = setup()
    transcript.load(snapshot([userMessage('u1', 'a'), userMessage('u2', 'b')]))
    transcript.apply({ type: 'removed', message_id: 'u2' }, 0)
    transcript.flush(0)
    expect(texts(transcript.blocks)).toEqual(['a'])
  })
})

describe('Transcript: streaming', () => {
  function streaming() {
    const env = setup()
    env.transcript.load(snapshot([userMessage('u1', 'go')], { info: info({ status: { state: 'busy' } }) }))
    env.transcript.flush(0)
    env.transcript.apply({ type: 'message', message: assistantMessage('a1', [textPart('')]) }, 0)
    return env
  }

  it('shows deltas gradually, not when they arrive', () => {
    const { transcript } = streaming()
    transcript.apply(textDelta('a1', 0, 'x'.repeat(100)), 0)

    transcript.flush(0)
    expect(shownOf(transcript)).toBe('')
    transcript.flush(100)
    const halfway = shownOf(transcript).length
    expect(halfway).toBeGreaterThan(30)
    expect(halfway).toBeLessThan(70)
    transcript.flush(LATER)
    expect(shownOf(transcript)).toBe('x'.repeat(100))
  })

  it('settle shows everything at once', () => {
    const { transcript } = streaming()
    transcript.apply(textDelta('a1', 0, 'hello world'), 0)
    transcript.settle()
    transcript.flush(0)
    expect(texts(transcript.blocks).at(-1)).toBe('hello world')
  })

  it('changes only the block that received text', () => {
    const { transcript } = streaming()
    transcript.apply({ type: 'part', message_id: 'a1', index: 1, part: toolPart('c1', 'done') }, 0)
    transcript.apply(textDelta('a1', 0, 'abc'), 0)
    transcript.flush(LATER)
    const before = [...transcript.blocks]

    transcript.apply(textDelta('a1', 0, 'def'), LATER)
    transcript.flush(LATER * 2)
    const after = transcript.blocks

    expect(after).toHaveLength(before.length)
    const changed = after.flatMap((block, i) => (block !== before[i] ? [block.key] : []))
    expect(changed).toEqual(['a1:0:0'])
  })

  it('returns false and keeps the array when nothing changed', () => {
    const { transcript } = streaming()
    transcript.flush(LATER)
    const blocks = transcript.blocks
    expect(transcript.flush(LATER + 16)).toBe(false)
    expect(transcript.blocks).toBe(blocks)
  })

  it('marks the last block of the last part as streaming while the session is busy', () => {
    const { transcript } = streaming()
    transcript.apply(textDelta('a1', 0, 'one\n\n```js\nx\n```\ntail'), 0)
    transcript.flush(LATER)
    const flags = content(transcript.blocks)
      .filter((b) => b.messageId === 'a1')
      .map((b) => b.streaming)
    expect(flags).toEqual([false, false, true])

    transcript.apply({ type: 'info', info: info({ status: { state: 'idle' } }) }, LATER)
    transcript.apply(
      {
        type: 'message',
        message: assistantMessage('a1', [textPart('one\n\n```js\nx\n```\ntail')], { time_end: 5 }),
      },
      LATER,
    )
    transcript.flush(LATER * 2)
    expect(transcript.blocks.every((b) => !b.streaming)).toBe(true)
  })

  it('keeps streaming until paced text is out, then shows the error', () => {
    const { transcript } = streaming()
    transcript.apply(textDelta('a1', 0, 'x'.repeat(100)), 0)
    transcript.apply(
      {
        type: 'message',
        message: assistantMessage('a1', [textPart('x'.repeat(100))], { time_end: 5, error: 'boom' }),
      },
      0,
    )
    transcript.apply({ type: 'info', info: info() }, 0)

    transcript.flush(50)
    expect(kinds(transcript.blocks).at(-1)).toBe('markdown')
    expect(transcript.blocks.at(-1)?.streaming).toBe(true)

    transcript.flush(LATER)
    transcript.flush(LATER + 16)
    expect(kinds(transcript.blocks).slice(-2)).toEqual(['markdown', 'error'])
    expect(transcript.blocks.at(-2)?.streaming).toBe(false)
  })

  it('does not show an interrupted turn as an error', () => {
    const { transcript } = streaming()
    transcript.apply(
      {
        type: 'message',
        message: assistantMessage('a1', [textPart('hi')], { time_end: 5, error: 'Aborted' }),
      },
      0,
    )
    transcript.flush(LATER)
    expect(kinds(transcript.blocks)).not.toContain('error')
  })

  it('shows rewritten text at once instead of pacing it', () => {
    const { transcript } = streaming()
    transcript.apply(textDelta('a1', 0, 'draft'), 0)
    transcript.flush(LATER)
    transcript.apply({ type: 'part', message_id: 'a1', index: 0, part: textPart('something else') }, LATER)
    transcript.flush(LATER)
    expect(texts(transcript.blocks).at(-1)).toBe('something else')
  })

  it('paces text a Part event adds to what was shown', () => {
    const { transcript } = streaming()
    transcript.apply(textDelta('a1', 0, 'abc'), 0)
    transcript.flush(LATER)
    transcript.apply(
      { type: 'part', message_id: 'a1', index: 0, part: textPart('abc' + 'd'.repeat(100)) },
      LATER,
    )
    transcript.flush(LATER)
    expect(texts(transcript.blocks).at(-1)).toBe('abc')
    transcript.flush(LATER * 2)
    expect(texts(transcript.blocks).at(-1)).toBe('abc' + 'd'.repeat(100))
  })

  it('builds a tool call from streamed arguments and reads the purpose early', () => {
    const { transcript } = streaming()
    transcript.apply(
      {
        type: 'part',
        message_id: 'a1',
        index: 1,
        part: { ...toolPart('c1', 'streaming'), purpose: '', raw_input: '', input: null },
      },
      0,
    )
    transcript.apply(
      {
        type: 'delta',
        message_id: 'a1',
        index: 1,
        delta: { type: 'tool_input', text: '{"purpose": "List fi' },
      },
      0,
    )
    transcript.flush(0)

    const tool = content(transcript.blocks).at(-1)
    expect(tool).toMatchObject({ kind: 'tool', state: 'streaming', purpose: 'List fi', streaming: true })
  })

  it('reports deltas that do not fit', () => {
    const { transcript, errors } = streaming()
    transcript.apply(
      { type: 'delta', message_id: 'a1', index: 0, delta: { type: 'reasoning', text: 'x' } },
      0,
    )
    transcript.apply({ type: 'delta', message_id: 'a1', index: 5, delta: { type: 'text', text: 'x' } }, 0)
    expect(errors).toHaveLength(2)
  })

  it('creates the part a delta starts', () => {
    const { transcript } = streaming()
    transcript.apply(
      { type: 'delta', message_id: 'a1', index: 1, delta: { type: 'reasoning', text: 'think' } },
      0,
    )
    transcript.flush(LATER)
    expect(kinds(transcript.blocks)).toEqual(['user', 'reasoning'])
  })
})

describe('Transcript: permissions', () => {
  it('attaches a pending request to its tool call until it is resolved', () => {
    const { transcript } = setup()
    transcript.load(snapshot([assistantMessage('a1', [toolPart('c1', 'awaiting_permission')])]))
    transcript.apply(
      {
        type: 'permission',
        request: {
          session_id: 's',
          message_id: 'a1',
          call_id: 'c1',
          tool: 'bash',
          purpose: 'p',
          input: {},
          time: 0,
        },
      },
      0,
    )
    transcript.flush(0)
    expect(transcript.blocks[0]).toMatchObject({ kind: 'tool', permission: { call_id: 'c1' } })

    transcript.apply({ type: 'permission_resolved', call_id: 'c1' }, 0)
    transcript.flush(0)
    expect(transcript.blocks[0]).toMatchObject({ kind: 'tool', permission: undefined })
  })
})

describe('Transcript: message footers', () => {
  it('ends every message with a meta block carrying time, model and tokens', () => {
    const { transcript } = setup()
    transcript.load(
      snapshot([
        userMessage('u1', 'hi'),
        assistantMessage('a1', [textPart('ok')], {
          time: 10,
          time_end: 1210,
          agent: 'build',
          model: { provider: 'p', model: 'm', variant: 'high' },
          usage: { input: 100, output: 20, cache_read: 30, cache_write: 4, reasoning: 0 },
        }),
      ]),
    )
    transcript.flush(0)
    const metas = transcript.blocks.filter((block) => block.kind === 'meta')
    expect(metas[0]).toMatchObject({ messageId: 'u1', role: 'user', canUndo: true })
    expect(metas[1]).toMatchObject({
      messageId: 'a1',
      timeEnd: 1210,
      agent: 'build',
      model: 'm',
      variant: 'high',
      tokensIn: 134,
      tokensOut: 20,
      interrupted: false,
    })
  })

  it('marks an interrupted reply instead of showing an error', () => {
    const { transcript } = setup()
    transcript.load(snapshot([assistantMessage('a1', [], { time_end: 1, error: 'Aborted' })]))
    transcript.flush(0)
    expect(transcript.blocks).toHaveLength(1)
    expect(transcript.blocks[0]).toMatchObject({ kind: 'meta', interrupted: true, failed: false })
  })

  it('lets only human prompts be undone', () => {
    const { transcript } = setup()
    transcript.load(snapshot([userMessage('u1', 'out', { kind: 'shell' })]))
    transcript.flush(0)
    expect(transcript.blocks[1]).toMatchObject({ kind: 'meta', canUndo: false })
  })
})

describe('Transcript: compaction', () => {
  const compacted = [
    userMessage('u1', 'old'),
    userMessage('c1', 'summary one', { kind: 'compaction' }),
    userMessage('u2', 'mid'),
    userMessage('c2', 'summary two', { kind: 'compaction' }),
    userMessage('u3', 'new'),
  ]

  it('draws compaction messages as one block and marks what came before the last as stale', () => {
    const { transcript } = setup()
    transcript.load(snapshot(compacted))
    transcript.flush(0)
    const stale = transcript.blocks.filter((block) => block.stale).map((block) => block.key)
    expect(stale).toEqual(['u1:user', 'u1:meta', 'c1:compaction', 'u2:user', 'u2:meta'])
    const dividers = transcript.blocks.filter((block) => block.kind === 'compaction')
    expect(dividers.map((block) => block.last)).toEqual([false, true])
  })

  it('refreshes the marks when a reset drops the last compaction', () => {
    const { transcript } = setup()
    transcript.load(snapshot(compacted))
    transcript.flush(0)
    transcript.apply({ type: 'reset', messages: compacted.slice(0, 3), more_before: false }, 0)
    transcript.flush(0)
    expect(transcript.blocks.filter((block) => block.kind === 'compaction')).toMatchObject([{ last: true }])
    expect(transcript.blocks.filter((block) => block.stale).map((block) => block.key)).toEqual([
      'u1:user',
      'u1:meta',
    ])
  })
})

describe('Transcript: reset and older pages', () => {
  it('keeps the blocks of the messages a reset leaves alone', () => {
    const { transcript } = setup()
    transcript.load(snapshot([userMessage('u1', 'a'), userMessage('u2', 'b')]))
    transcript.flush(0)
    const before = transcript.blocks
    transcript.apply({ type: 'reset', messages: [userMessage('u1', 'a')], more_before: false }, 0)
    transcript.flush(0)
    expect(transcript.blocks).toHaveLength(2)
    expect(transcript.blocks[0]).toBe(before[0])
    expect(transcript.blocks[1]).toBe(before[1])
  })

  it('puts an older page above and moves the first turn divider', () => {
    const { transcript } = setup()
    transcript.load(snapshot([userMessage('u2', 'b')], { more_before: true }))
    transcript.flush(0)
    expect(transcript.oldestMessageId).toBe('u2')
    transcript.prepend([userMessage('u1', 'a')], false)
    transcript.flush(0)
    expect(transcript.oldestMessageId).toBe('u1')
    const opens = transcript.blocks.filter((block) => block.kind === 'user').map((block) => block.opensTurn)
    expect(opens).toEqual([false, true])
  })
})

describe('Transcript: session error', () => {
  it('shows the last error after the messages unless the last message has it', () => {
    const { transcript } = setup()
    transcript.load(snapshot([assistantMessage('a1', [], { time_end: 1, error: 'boom' })]))
    transcript.apply({ type: 'error', text: 'boom' }, 0)
    transcript.flush(0)
    expect(transcript.blocks.filter((block) => block.kind === 'error')).toHaveLength(1)
    transcript.apply({ type: 'error', text: 'later' }, 0)
    transcript.flush(0)
    expect(transcript.blocks.at(-1)).toMatchObject({ kind: 'error', messageId: '', text: 'later' })
    transcript.clearError()
    transcript.flush(0)
    expect(transcript.blocks.at(-1)?.kind).toBe('meta')
  })

  it('keeps the error through message and reset events', () => {
    const { transcript } = setup()
    transcript.load(snapshot([userMessage('u1', 'a')]))
    transcript.apply({ type: 'error', text: 'boom' }, 0)
    transcript.apply({ type: 'reset', messages: [], more_before: false }, 0)
    transcript.flush(0)
    expect(transcript.blocks).toMatchObject([{ kind: 'error', text: 'boom' }])
  })

  it('knows whether the newest tool call waits for its delay', () => {
    const { transcript } = setup()
    transcript.load(snapshot([assistantMessage('a1', [toolPart('c1', 'delayed')])]))
    expect(transcript.lastToolDelayed).toBe(true)
  })
})
