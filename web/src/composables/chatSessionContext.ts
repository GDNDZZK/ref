// The session a component belongs to. SessionPane provides it, so the parts of a conversation
// (tool blocks, permission buttons) need not be handed the session through every level.

import { inject } from 'vue'
import type { InjectionKey } from 'vue'
import type { ChatSession } from '../model/chatSession.ts'

export const chatSessionKey: InjectionKey<ChatSession> = Symbol('chatSession')

export function useChatSession(): ChatSession {
  const session = inject(chatSessionKey)
  if (!session) throw new Error('this component belongs inside a SessionPane')
  return session
}
