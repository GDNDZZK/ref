// What the window shows: the live sessions, which one is open, and the actions on them.
//
//   frames --> Workspace --event--> ChatSession (one per session opened in this window)
//                 |--sessions--> live (the sidebar's list)
//
// A ChatSession stays while the session lives, so switching away only detaches it.

import { defineStore } from 'pinia'
import { computed, ref, shallowRef } from 'vue'
import { ChatSession } from '../model/chatSession.ts'
import type { EventFrame } from '../net/connection.ts'
import type { ModelRef, SessionInfo } from '../proto/generated'
import { useConnectionStore } from './connection.ts'
import { emitDraftRestored } from './draftBus.ts'
import { useNoticesStore } from './notices.ts'

export interface NewSessionOptions {
  cwd?: string // default: the workspace's directory
  agent?: string
  model?: ModelRef
  name?: string
}

// Live sessions in the order they were created, which is the order of the sidebar.
export function sortLive(sessions: readonly SessionInfo[]): SessionInfo[] {
  return [...sessions].sort((a, b) => a.created_at - b.created_at || a.id.localeCompare(b.id))
}

const ERROR_TOAST_MAX_CHARS = 200

export const useWorkspaceStore = defineStore('workspace', () => {
  const connection = useConnectionStore()
  const notices = useNoticesStore()

  const live = shallowRef<readonly SessionInfo[]>([])
  const currentId = ref<string>()
  const cwd = ref('') // where new sessions start

  const chats = new Map<string, ChatSession>()

  const current = computed(() => (currentId.value ? chat(currentId.value) : undefined))

  // The session's object; made on first use, without any request.
  function chat(id: string): ChatSession {
    const existing = chats.get(id)
    if (existing) return existing
    const info = live.value.find((candidate) => candidate.id === id)
    const created = new ChatSession(
      id,
      {
        api: () => connection.getApi(),
        notify: (level, text) => notices.push(level, text),
        sessionError,
        draftRestored: emitDraftRestored,
      },
      info,
    )
    chats.set(id, created)
    return created
  }

  // The open session shows its error in the conversation; any other one says which it is about.
  function sessionError(session: ChatSession, text: string): void {
    if (session.id === currentId.value) return
    const line = text.split('\n')[0]!.slice(0, ERROR_TOAST_MAX_CHARS)
    notices.push('error', `${session.info?.name ?? session.id}: ${line}`)
  }

  // Makes `id` the open session, attached in the foreground. The one shown before is detached.
  async function openSession(id: string): Promise<void> {
    const previousId = currentId.value
    await chat(id).attach(true)
    currentId.value = id
    if (previousId === undefined || previousId === id) return
    await chat(previousId).detach()
  }

  async function newSession(options: NewSessionOptions = {}): Promise<string> {
    const info = await connection.getApi().createSession(options.cwd ?? cwd.value, options)
    live.value = sortLive([...live.value.filter((session) => session.id !== info.id), info])
    await openSession(info.id)
    return info.id
  }

  function setLive(sessions: readonly SessionInfo[]): void {
    live.value = sortLive(sessions)
    for (const info of sessions) chats.get(info.id)?.setInfo(info)
  }

  function onFrame(frame: EventFrame): void {
    if (frame.kind === 'sessions') setLive(frame.sessions)
    if (frame.kind === 'event') chats.get(frame.session_id)?.applyEvent(frame.event)
  }

  // Called when the connection opens. The first time, opens `sessionId` or a new session in `startCwd`.
  async function onConnected(
    isReconnect: boolean,
    start: { cwd: string; sessionId?: string },
  ): Promise<void> {
    if (isReconnect) {
      setLive(await connection.getApi().listSessions())
      await Promise.all([...chats.values()].map((session) => session.resync()))
      return
    }
    cwd.value = start.cwd
    await connection.loadConfig()
    setLive(await connection.getApi().listSessions())
    if (start.sessionId) {
      try {
        await openSession(start.sessionId)
        return
      } catch (error) {
        notices.pushError(error)
      }
    }
    await newSession()
  }

  return { live, currentId, current, cwd, chat, openSession, newSession, onFrame, onConnected }
})
