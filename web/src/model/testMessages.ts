// Builders for tests: the smallest valid messages and sessions.

import { vi } from 'vitest'
import type { RefApi } from '../net/api.ts'
import type {
  Message,
  Part,
  Question,
  QuestionRequest,
  SessionInfo,
  SessionSnapshot,
} from '../proto/generated'
import { ChatSession } from './chatSession.ts'

export function info(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    id: 'ses_1',
    name: 'test',
    title: 'Test',
    agent: 'build',
    model: { provider: 'p', model: 'm' },
    cwd: '/work',
    created_at: 0,
    updated_at: 0,
    status: { state: 'idle' },
    attached: 1,
    live: true,
    parent: null,
    must_report: false,
    context: { used: 0, max: 0, recommend: 0, auto: 0 },
    ...overrides,
  }
}

export function userMessage(
  id: string,
  text: string,
  source: Message['source'] = { kind: 'human' },
): Message {
  return {
    id,
    role: 'user',
    time: 0,
    time_end: 0,
    source,
    parts: [{ type: 'text', text, time: 0 }],
    model: null,
    agent: null,
    usage: null,
    error: null,
  }
}

export function assistantMessage(id: string, parts: Part[] = [], overrides: Partial<Message> = {}): Message {
  return {
    id,
    role: 'assistant',
    time: 0,
    time_end: null,
    source: { kind: 'human' },
    parts,
    model: null,
    agent: null,
    usage: null,
    error: null,
    ...overrides,
  }
}

export const textPart = (text: string): Part => ({ type: 'text', text, time: 0 })
export const reasoningPart = (text: string): Part => ({ type: 'reasoning', text, time: 0, time_end: null })

type ToolPart = Extract<Part, { type: 'tool' }>

export function toolPart(callId: string, state: ToolPart['state'] = 'running'): ToolPart {
  return {
    type: 'tool',
    call_id: callId,
    name: 'bash',
    purpose: 'Run it',
    input: { command: 'ls' },
    raw_input: '{"command":"ls"}',
    state,
    output: null,
    time: 0,
    time_end: null,
  }
}

export function snapshot(messages: Message[], overrides: Partial<SessionSnapshot> = {}): SessionSnapshot {
  return {
    info: info(),
    messages,
    more_before: false,
    pending_permissions: [],
    pending_questions: [],
    queued: [],
    ...overrides,
  }
}

export function question(overrides: Partial<Question> = {}): Question {
  return {
    question: 'Which one?',
    header: 'Pick',
    multiple: false,
    custom: true,
    options: [
      { label: 'A', description: 'first', default: false },
      { label: 'B', description: 'second', default: true },
    ],
    ...overrides,
  }
}

export function questionRequest(
  questions: Question[] = [question()],
  overrides: Partial<QuestionRequest> = {},
): QuestionRequest {
  return { session_id: 'ses_1', message_id: 'a1', call_id: 'c1', time: 1, questions, ...overrides }
}

// A session on a stubbed api whose callbacks do nothing.
export function chatSessionWith(api: object, id = 'ses_1'): ChatSession {
  return new ChatSession(id, {
    api: () => api as unknown as RefApi,
    notify: vi.fn(),
    draftRestored: vi.fn(),
  })
}
