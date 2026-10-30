// Answers the oldest pending permission request from the keyboard: a = allow, s = allow for the
// session, d = deny. Typing in the composer is left alone. Works on the focused session, or any
// session on screen that is waiting.

import { onBeforeUnmount, onMounted } from 'vue'
import { isTyping } from './isTyping.ts'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'

const ANSWERS: Record<string, { allow: boolean; always: boolean }> = {
  a: { allow: true, always: false },
  s: { allow: true, always: true },
  d: { allow: false, always: false },
}

export function usePermissionKeys(): void {
  const workspace = useWorkspaceStore()
  const notices = useNoticesStore()

  // The focused session, or another one on screen that is waiting for an answer.
  function askingSession() {
    const ids = [workspace.focusedId, ...workspace.visibleSessionIds]
    const sessions = ids.flatMap((id) => (id === undefined ? [] : [workspace.chat(id)]))
    return sessions.find((session) => session.permissions.length > 0)
  }

  function onKeydown(event: KeyboardEvent): void {
    const answer = ANSWERS[event.key]
    if (!answer || isTyping(event.target)) return
    const session = askingSession()
    const request = session?.permissions[0]
    if (!session || !request) return
    if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return
    event.preventDefault()
    session
      .answerPermission(request.call_id, answer.allow, answer.always)
      .catch((error) => notices.pushError(error))
  }

  onMounted(() => window.addEventListener('keydown', onKeydown))
  onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
}
