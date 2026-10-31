import { describe, expect, it, vi } from 'vitest'
import type { RefApi } from '../net/api.ts'
import type { QuestionRequest } from '../proto/generated'
import { ChatSession } from './chatSession.ts'
import { assistantMessage, info, snapshot, toolPart, userMessage } from './testMessages.ts'

const FRAME_WAIT_MS = 60 // the session shows changes on the next animation frame
const afterFrame = () => new Promise((resolve) => setTimeout(resolve, FRAME_WAIT_MS))

const REQUEST: QuestionRequest = {
  session_id: 'ses_1',
  message_id: 'a1',
  call_id: 'c1',
  time: 1,
  questions: [
    {
      question: 'Which one?',
      header: 'Pick',
      multiple: false,
      custom: true,
      options: [
        { label: 'A', description: 'first', default: false },
        { label: 'B', description: 'second', default: true },
      ],
    },
  ],
}

async function setup(questionReply = vi.fn().mockResolvedValue(undefined)) {
  const messages = [userMessage('u1', 'hi'), assistantMessage('a1', [toolPart('c1')])]
  const api = {
    attach: vi.fn().mockResolvedValue(snapshot(messages, { pending_questions: [REQUEST] })),
    detach: vi.fn(),
    questionReply,
  }
  const session = new ChatSession('ses_1', {
    api: () => api as unknown as RefApi,
    notify: vi.fn(),
    draftRestored: vi.fn(),
  })
  await session.attach(true)
  return { api, questionReply, session }
}

function toolBlockOf(session: ChatSession) {
  const block = session.blocks.find((candidate) => candidate.kind === 'tool')
  if (block?.kind !== 'tool') throw new Error('no tool block')
  return block
}

describe('questions of a session', () => {
  it('knows the pending question from the snapshot and shows it on the tool call', async () => {
    const { session } = await setup()
    expect(session.pendingQuestion?.call_id).toBe('c1')
    expect(toolBlockOf(session).question?.call_id).toBe('c1')
  })

  it('takes a question and its end from events', async () => {
    const { session } = await setup()
    session.applyEvent({ type: 'question_resolved', call_id: 'c1' })
    await afterFrame()
    expect(session.pendingQuestion).toBeUndefined()
    expect(toolBlockOf(session).question).toBeUndefined()

    session.applyEvent({ type: 'question', request: REQUEST })
    await afterFrame()
    expect(session.pendingQuestion?.call_id).toBe('c1')
    expect(toolBlockOf(session).question?.call_id).toBe('c1')
  })

  it('keeps the choices of a question while it is pending, and drops them after', async () => {
    const { session } = await setup()
    const form = session.questionForm(REQUEST)
    form.select(0, 0)
    expect(session.questionForm(REQUEST)).toBe(form)
    session.applyEvent({ type: 'question_resolved', call_id: 'c1' })
    expect(session.questionForm(REQUEST)).not.toBe(form)
  })

  it('reports the waiting status as the activity', async () => {
    const { session } = await setup()
    session.applyEvent({ type: 'info', info: info({ status: { state: 'waiting_question' } }) })
    expect(session.activity).toBe('waiting_question')
  })
})

describe('answering a question', () => {
  it('sends the answers once and locks the card until the backend reports it resolved', async () => {
    const { session, questionReply } = await setup()
    const first = session.answerQuestion('c1', [['B']])
    expect(session.isSendingAnswer('c1')).toBe(true)
    expect(session.pendingQuestion).toBeUndefined() // the keys no longer act on it
    await session.answerQuestion('c1', [['A']])
    await first
    expect(questionReply).toHaveBeenCalledTimes(1)
    expect(questionReply).toHaveBeenCalledWith('ses_1', 'c1', [['B']])
    expect(toolBlockOf(session).question).toBeDefined() // still there, locked

    session.applyEvent({ type: 'question_resolved', call_id: 'c1' })
    await afterFrame()
    expect(session.isSendingAnswer('c1')).toBe(false)
    expect(toolBlockOf(session).question).toBeUndefined()
  })

  it('sends null to dismiss', async () => {
    const { session, questionReply } = await setup()
    await session.answerQuestion('c1', null)
    expect(questionReply).toHaveBeenCalledWith('ses_1', 'c1', null)
  })

  it('unlocks the card and reports an answer the backend refused', async () => {
    const refused = vi.fn().mockRejectedValue(new Error('Question 1 takes one answer.'))
    const { session } = await setup(refused)
    await expect(session.answerQuestion('c1', [['A', 'B']])).rejects.toThrow('takes one answer')
    expect(session.isSendingAnswer('c1')).toBe(false)
    expect(session.pendingQuestion?.call_id).toBe('c1')
  })

  it('drops a question that was answered elsewhere without a word', async () => {
    const gone = vi.fn().mockRejectedValue(new Error('No pending question with that id.'))
    const { session } = await setup(gone)
    await session.answerQuestion('c1', [['A']])
    await afterFrame()
    expect(session.pendingQuestion).toBeUndefined()
    expect(session.isSendingAnswer('c1')).toBe(false)
    expect(toolBlockOf(session).question).toBeUndefined()
  })
})
