import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { PERMISSION_GUARD_MS } from '../components/toolLimits.ts'
import { ChatSession } from '../model/chatSession.ts'
import { assistantMessage, snapshot, toolPart, userMessage } from '../model/testMessages.ts'
import type { RefApi } from '../net/api.ts'
import type { Question, QuestionRequest } from '../proto/generated'
import { usePermissionKeys } from './usePermissionKeys.ts'

const pushError = vi.fn()
let session: ChatSession

vi.mock('../stores/workspace.ts', () => ({
  useWorkspaceStore: () => ({
    focusedId: 'ses_1',
    visibleSessionIds: ['ses_1'],
    chat: () => session,
  }),
}))
vi.mock('../stores/notices.ts', () => ({ useNoticesStore: () => ({ pushError }) }))

function question(overrides: Partial<Question> = {}): Question {
  return {
    question: 'Which one?',
    header: 'Pick',
    multiple: false,
    custom: true,
    options: [
      { label: 'A', description: '', default: false },
      { label: 'B', description: '', default: true },
    ],
    ...overrides,
  }
}

async function ask(...questions: Question[]) {
  const request: QuestionRequest = {
    session_id: 'ses_1',
    message_id: 'a1',
    call_id: 'c1',
    questions,
    time: 1,
  }
  const questionReply = vi.fn().mockResolvedValue(undefined)
  const api = {
    attach: vi.fn().mockResolvedValue(
      snapshot([userMessage('u1', 'hi'), assistantMessage('a1', [toolPart('c1')])], {
        pending_questions: [request],
      }),
    ),
    questionReply,
  }
  session = new ChatSession('ses_1', {
    api: () => api as unknown as RefApi,
    notify: vi.fn(),
    draftRestored: vi.fn(),
  })
  await session.attach(true)
  const host = mount(
    defineComponent({
      setup() {
        usePermissionKeys()
        return () => h('div')
      },
    }),
    { attachTo: document.body },
  )
  await flushPromises()
  return { questionReply, host }
}

function press(
  key: string,
  target: EventTarget = document.body,
  init: KeyboardEventInit = {},
): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init })
  target.dispatchEvent(event)
  return event
}

beforeEach(() => vi.useFakeTimers())
afterEach(() => {
  vi.useRealTimers()
  document.body.innerHTML = ''
})

describe('keys for a pending question', () => {
  it('takes the default with Enter once the guard time has passed', async () => {
    const { questionReply, host } = await ask(question())
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS - 1)
    press('Enter')
    expect(questionReply).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(2)
    const event = press('Enter')
    expect(event.defaultPrevented).toBe(true)
    expect(questionReply).toHaveBeenCalledWith('ses_1', 'c1', [['B']])
    host.unmount()
  })

  it('picks with a number and moves with the arrows', async () => {
    const { questionReply, host } = await ask(question())
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS)
    press('ArrowUp')
    press('Enter')
    expect(questionReply).toHaveBeenCalledWith('ses_1', 'c1', [['A']])
    host.unmount()
  })

  it('dismisses with Esc', async () => {
    const { questionReply, host } = await ask(question())
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS)
    press('Escape')
    expect(questionReply).toHaveBeenCalledWith('ses_1', 'c1', null)
    host.unmount()
  })

  it('keeps letters away from the composer but lets other keys through', async () => {
    const { questionReply, host } = await ask(question())
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS)
    expect(press('x').defaultPrevented).toBe(true)
    expect(press('F5').defaultPrevented).toBe(false)
    expect(press('c', document.body, { ctrlKey: true }).defaultPrevented).toBe(false)
    expect(questionReply).not.toHaveBeenCalled()
    host.unmount()
  })

  it('leaves the keys of the own-answer field to the field', async () => {
    const { questionReply, host } = await ask(question())
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS)
    const field = document.createElement('input')
    field.setAttribute('data-question-field', '')
    document.body.append(field)
    expect(press('Enter', field).defaultPrevented).toBe(false)
    expect(press('a', field).defaultPrevented).toBe(false)
    expect(questionReply).not.toHaveBeenCalled()
    host.unmount()
  })

  it('walks the tabs of several questions and submits on the confirm tab', async () => {
    const { questionReply, host } = await ask(question(), question({ header: 'Two' }))
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS)
    press('Enter') // takes B and moves to the second question
    press('1') // takes A and moves to the confirm tab
    expect(questionReply).not.toHaveBeenCalled()
    press('Enter')
    expect(questionReply).toHaveBeenCalledWith('ses_1', 'c1', [['B'], ['A']])
    host.unmount()
  })
})
