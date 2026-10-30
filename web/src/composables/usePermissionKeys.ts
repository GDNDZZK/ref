// Answers the oldest pending permission request from the keyboard: a or Enter = allow, s = allow
// for the session, d or Esc = deny. Works on the focused session, or any session on screen that
// is waiting.
//
// The keys are ignored for a moment after a request appears, because the user may still be typing.
// After that the request owns the keyboard: other letters no longer reach the composer until
// every request is answered. Dialogs keep their keys.

import { onBeforeUnmount, onMounted, watch } from 'vue'
import { PERMISSION_GUARD_MS } from '../components/toolLimits.ts'
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

export function usePermissionKeys(): void {
  const workspace = useWorkspaceStore()
  const notices = useNoticesStore()
  let armedAt = 0

  // The focused session, or another one on screen that is waiting for an answer.
  function askingSession() {
    const ids = [workspace.focusedId, ...workspace.visibleSessionIds]
    const sessions = ids.flatMap((id) => (id === undefined ? [] : [workspace.chat(id)]))
    return sessions.find((session) => session.permissions.length > 0)
  }

  // Each new request starts its own guard time.
  watch(
    () => askingSession()?.pendingPermission?.call_id,
    (callId) => callId && (armedAt = performance.now() + PERMISSION_GUARD_MS),
    { immediate: true },
  )

  function onKeydown(event: KeyboardEvent): void {
    const session = askingSession()
    const request = session?.pendingPermission
    if (!session || !request || performance.now() < armedAt || inDialog(event.target)) return
    if (event.ctrlKey || event.metaKey || event.altKey) return

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
