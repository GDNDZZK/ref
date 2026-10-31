// Answers the oldest pending permission request from the keyboard: a or Enter = allow, s = allow
// for the session, d or Esc = deny. A pending question takes the keys of its form instead
// (questionForm.ts): arrows, numbers, Enter, Esc. The keys only act on the focused pane (the
// zoomed tile, or the focused one in split view): a card in another tile is answered with its
// buttons, or by focusing that tile first, so typing in one tile never answers for another.
//
// The keys are ignored for a moment after a request appears, because the user may still be typing.
// After that the request owns the keyboard: other letters no longer reach the composer until
// every request is answered. Dialogs keep their keys, and so does the text field of a question.

import { onBeforeUnmount, onMounted, watch } from 'vue'
import { PERMISSION_GUARD_MS } from '../components/toolLimits.ts'
import type { ChatSession } from '../model/chatSession.ts'
import type { QuestionRequest } from '../proto/generated'
import { useNoticesStore } from '../stores/notices.ts'
import { useWorkspaceStore } from '../stores/workspace.ts'

const ANSWERS: Record<string, { allow: boolean; always: boolean }> = {
  a: { allow: true, always: false },
  Enter: { allow: true, always: false },
  s: { allow: true, always: true },
  d: { allow: false, always: false },
  Escape: { allow: false, always: false },
}

function inDialog(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest('[role="dialog"]') !== null
}

// The field for an own answer handles its keys itself.
function inQuestionField(target: EventTarget | null): boolean {
  return target instanceof HTMLInputElement && target.hasAttribute('data-question-field')
}

export function usePermissionKeys(): void {
  const workspace = useWorkspaceStore()
  const notices = useNoticesStore()
  let armedAt = 0

  // The focused session, when it is waiting for an answer.
  function askingSession() {
    const id = workspace.focusedId
    if (id === undefined) return undefined
    const session = workspace.chat(id)
    return session.permissions.length > 0 || session.pendingQuestion ? session : undefined
  }

  // Each new request starts its own guard time.
  watch(
    () => {
      const session = askingSession()
      return session?.pendingPermission?.call_id ?? session?.pendingQuestion?.call_id
    },
    (callId) => callId && (armedAt = performance.now() + PERMISSION_GUARD_MS),
    { immediate: true },
  )

  function answerQuestion(session: ChatSession, request: QuestionRequest, event: KeyboardEvent): void {
    const outcome = session.questionForm(request).press(event.key, event.shiftKey)
    if (outcome === 'ignored') {
      if (event.key.length === 1) event.preventDefault() // a letter: not for the composer now
      return
    }
    event.preventDefault()
    event.stopPropagation()
    if (outcome === 'handled') return
    const answers = outcome === 'submit' ? session.questionForm(request).answers() : null
    session.answerQuestion(request.call_id, answers).catch((error) => notices.pushError(error))
  }

  function onKeydown(event: KeyboardEvent): void {
    const session = askingSession()
    const request = session?.pendingPermission
    const question = session?.pendingQuestion
    if (!session || performance.now() < armedAt || inDialog(event.target)) return
    if (event.ctrlKey || event.metaKey || event.altKey) return
    if (!request) {
      if (question && !inQuestionField(event.target)) answerQuestion(session, question, event)
      return
    }

    const answer = ANSWERS[event.key]
    if (!answer) {
      if (event.key.length === 1) event.preventDefault() // a letter: not for the composer now
      return
    }
    event.preventDefault()
    event.stopPropagation()
    session
      .answerPermission(request.call_id, answer.allow, answer.always)
      .catch((error) => notices.pushError(error))
  }

  // Capture phase: before the composer sees the key.
  onMounted(() => window.addEventListener('keydown', onKeydown, true))
  onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown, true))
}
