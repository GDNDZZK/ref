// What blocks need from the conversation view, passed down with provide/inject.

import { inject } from 'vue'
import type { InjectionKey, Ref } from 'vue'

export interface ChatScroll {
  scroller: Readonly<Ref<HTMLElement | null>>
  // Folding a body under a pinned header would carry the header up out of view with the end of
  // its block. While held, the view scrolls back as the block shrinks, so the header stays where
  // it is. Release once the fold is done.
  holdHeader(block: HTMLElement, headerHeight: number): void
  releaseHeader(): void
}

export const chatScrollKey: InjectionKey<ChatScroll> = Symbol('chatScroll')

export function useChatScroll(): ChatScroll {
  const chatScroll = inject(chatScrollKey)
  if (!chatScroll) throw new Error('a block needs a ChatView above it')
  return chatScroll
}
