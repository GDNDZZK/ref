// Answers the oldest pending permission request from the keyboard: a = allow, s = allow for the
// session, d = deny. Typing in the composer is left alone.

import { onBeforeUnmount, onMounted } from 'vue'
import { useConversationStore } from '../stores/conversation.ts'
import { useNoticesStore } from '../stores/notices.ts'

const ANSWERS: Record<string, { allow: boolean; always: boolean }> = {
  a: { allow: true, always: false },
  s: { allow: true, always: true },
  d: { allow: false, always: false },
}

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ['INPUT', 'TEXTAREA'].includes(target.tagName))
  )
}

export function usePermissionKeys(): void {
  const conversation = useConversationStore()
  const notices = useNoticesStore()

  function onKeydown(event: KeyboardEvent): void {
    const answer = ANSWERS[event.key]
    const request = conversation.permissions[0]
    if (!answer || !request || isTyping(event.target)) return
    if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return
    event.preventDefault()
    conversation
      .answerPermission(request.call_id, answer.allow, answer.always)
      .catch((error) => notices.pushError(error))
  }

  onMounted(() => window.addEventListener('keydown', onKeydown))
  onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
}
