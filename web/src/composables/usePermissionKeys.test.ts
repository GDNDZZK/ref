import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, reactive } from 'vue'
import { PERMISSION_GUARD_MS } from '../components/toolLimits.ts'
import { ChatSession } from '../model/chatSession.ts'
import { assistantMessage, snapshot, toolPart, userMessage } from '../model/testMessages.ts'
import type { RefApi } from '../net/api.ts'
import type { Question, QuestionRequest } from '../proto/generated'
import { usePermissionKeys } from './usePermissionKeys.ts'

const pushError = vi.fn()
const sessions = new Map<string, ChatSession>()
const workspaceStub = reactive({ focusedId: 'ses_1', visibleSessionIds: ['ses_1', 'ses_2'] })

vi.mock('../stores/workspace.ts', () => ({
  useWorkspaceStore: () => ({
    get focusedId() {
      return workspaceStub.focusedId
    },
    get visibleSessionIds() {
      return workspaceStub.visibleSessionIds
    },
    chat: (id: string) => sessions.get(id),
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

// Opens a session that has a pending question; `id` is where the question is asked.
async function openSession(id: string, questions: Question[]) {
  const request: QuestionRequest = {
    session_id: id,
    message_id: 'a1',
    call_id: 'c1',
    questions,
    time: 1,
  }
  const questionReply = vi.fn().mockResolvedValue(undefined)
  const api = {
    attach: vi.fn().mockResolvedValue(
      snapshot([userMessage('u1', 'hi'), assistantMessage('a1', [toolPart('c1')])], {
        pending_questions: questions.length > 0 ? [request] : [],
      }),
    ),
    questionReply,
  }
  const session = new ChatSession(id, {
    api: () => api as unknown as RefApi,
    notify: vi.fn(),
    draftRestored: vi.fn(),
  })
  await session.attach(true)
  sessions.set(id, session)
  return { questionReply, session }
}

async function ask(...questions: Question[]) {
  workspaceStub.focusedId = 'ses_1'
  const { questionReply } = await openSession('ses_1', questions)
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
  sessions.clear()
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

describe('keys in split view', () => {
  it('do not answer a question in a tile that is not focused', async () => {
    const { questionReply, host } = await ask(question())
    const other = await openSession('ses_2', [])
    workspaceStub.focusedId = 'ses_2'
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS * 2)
    expect(press('Enter').defaultPrevented).toBe(false)
    expect(press('Escape').defaultPrevented).toBe(false)
    expect(press('x').defaultPrevented).toBe(false) // typing in the focused tile goes on
    expect(questionReply).not.toHaveBeenCalled()
    expect(other.questionReply).not.toHaveBeenCalled()
    host.unmount()
  })

  it('answer once the tile with the question is focused and the guard time has passed', async () => {
    const { questionReply, host } = await ask(question())
    await openSession('ses_2', [])
    workspaceStub.focusedId = 'ses_2'
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS)
    workspaceStub.focusedId = 'ses_1'
    await flushPromises()
    press('Enter') // just focused: the guard time starts again
    expect(questionReply).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS)
    press('Enter')
    expect(questionReply).toHaveBeenCalledWith('ses_1', 'c1', [['B']])
    host.unmount()
  })

  it('do not answer a permission request in a tile that is not focused', async () => {
    const permissionReply = vi.fn().mockResolvedValue(undefined)
    const request = {
      session_id: 'ses_1',
      message_id: 'a1',
      call_id: 'p1',
      tool: 'bash',
      purpose: '',
      input: {},
      time: 1,
    }
    const api = {
      attach: vi
        .fn()
        .mockResolvedValue(snapshot([userMessage('u1', 'hi')], { pending_permissions: [request] })),
      permissionReply,
    }
    const waiting = new ChatSession('ses_1', {
      api: () => api as unknown as RefApi,
      notify: vi.fn(),
      draftRestored: vi.fn(),
    })
    await waiting.attach(true)
    sessions.set('ses_1', waiting)
    await openSession('ses_2', [])
    workspaceStub.focusedId = 'ses_2'
    const host = mount(
      defineComponent({
        setup() {
          usePermissionKeys()
          return () => h('div')
        },
      }),
    )
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS * 2)
    press('a')
    press('Enter')
    expect(permissionReply).not.toHaveBeenCalled()
    workspaceStub.focusedId = 'ses_1'
    await vi.advanceTimersByTimeAsync(PERMISSION_GUARD_MS)
    press('a')
    expect(permissionReply).toHaveBeenCalledWith('ses_1', 'p1', true, false)
    host.unmount()
  })
})
