// What the window shows: the live sessions, which one is open, and the actions on them.
//
//   frames --> Workspace --event--> ChatSession (one per session opened in this window)
//                 |--sessions--> live (the sidebar's list)
//
// A ChatSession stays while the session lives, so switching away only detaches it.

import { defineStore } from 'pinia'
import { computed, ref, shallowRef, watch } from 'vue'
import { ChatSession } from '../model/chatSession.ts'
import type { EventFrame } from '../net/connection.ts'
import { writeLocation } from '../net/location.ts'
import type { Startup } from '../net/startup.ts'
import type { ModelRef, SessionInfo } from '../proto/generated'
import { useConnectionStore } from './connection.ts'
import { useNoticesStore } from './notices.ts'

export interface NewSessionOptions {
  cwd?: string // default: the workspace's directory
  agent?: string
  model?: ModelRef
  name?: string
}

const HISTORY_LIMIT = 200
const ERROR_TOAST_MAX_CHARS = 200

// Live sessions in the order they were created, which is the order of the sidebar.
export function sortLive(sessions: readonly SessionInfo[]): SessionInfo[] {
  return [...sessions].sort((a, b) => a.created_at - b.created_at || a.id.localeCompare(b.id))
}

// "name: first line of the error", cut at 200 characters (the TUI's toast for a session in the background).
export function errorToast(name: string, text: string): string {
  const firstLine = text.trim().split('\n')[0] ?? ''
  const full = `${name}: ${firstLine}`
  return full.length <= ERROR_TOAST_MAX_CHARS ? full : `${full.slice(0, ERROR_TOAST_MAX_CHARS - 1)}…`
}

export const useWorkspaceStore = defineStore('workspace', () => {
  const connection = useConnectionStore()
  const notices = useNoticesStore()

  const live = shallowRef<readonly SessionInfo[]>([])
  const history = shallowRef<readonly SessionInfo[]>([])
  const historyLoaded = ref(false)
  const currentId = ref<string>()
  const cwd = ref('') // where new sessions start

  const chats = new Map<string, ChatSession>()

  const current = computed(() => (currentId.value ? chat(currentId.value) : undefined))
  // The session commands act on when they are not given one: the focused tile in split view.
  const focusedId = computed(() => currentId.value)

  watch(currentId, (id) => writeLocation({ sessionId: id }))

  // The session's object; made on first use, without any request.
  function chat(id: string): ChatSession {
    const existing = chats.get(id)
    if (existing) return existing
    const info = live.value.find((candidate) => candidate.id === id)
    const created = new ChatSession(
      id,
      { api: () => connection.getApi(), notify: (level, text) => notices.push(level, text) },
      info,
    )
    chats.set(id, created)
    return created
  }

  function infoOf(id: string): SessionInfo | undefined {
    return chats.get(id)?.info ?? live.value.find((session) => session.id === id)
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

  // A session is gone (closed here, by another client, or by the backend). When it was the open one,
  // the oldest live session takes over; when none is left, a new one starts in the same directory.
  async function discard(id: string, lastCwd: string): Promise<void> {
    const known = chats.has(id) || live.value.some((session) => session.id === id)
    if (!known) return // already handled: the closed event and the response of Close both end here
    chats.get(id)?.dispose()
    chats.delete(id)
    live.value = live.value.filter((session) => session.id !== id)
    void refreshHistory()
    if (currentId.value !== id) return
    currentId.value = undefined
    const next = live.value[0]
    if (next) await openSession(next.id)
    else await newSession({ cwd: lastCwd })
  }

  async function closeSession(id: string): Promise<void> {
    const lastCwd = infoOf(id)?.cwd ?? cwd.value
    await connection.getApi().close(id)
    await discard(id, lastCwd)
  }

  async function renameSession(id: string, name: string): Promise<void> {
    showInfo(await connection.getApi().rename(id, name))
  }

  async function deleteHistory(id: string): Promise<void> {
    await connection.getApi().deleteSession(id)
    history.value = history.value.filter((session) => session.id !== id)
  }

  async function setSessionCwd(id: string, path: string): Promise<void> {
    const info = await connection.getApi().setCwd(id, path)
    showInfo(info)
    cwd.value = info.cwd // new sessions of this window start there too
  }

  async function setModel(id: string, model: ModelRef): Promise<void> {
    showInfo(await connection.getApi().setModel(id, model))
  }

  async function setAgent(id: string, agent: string): Promise<void> {
    showInfo(await connection.getApi().setAgent(id, agent))
  }

  // An empty `variant` rotates to the model's next one.
  async function setVariant(id: string, variant: string): Promise<void> {
    showInfo(await connection.getApi().setVariant(id, variant === '' ? undefined : variant))
  }

  // To the next agent (`direction` 1) or the previous one (-1), wrapping around.
  async function cycleAgent(id: string, direction: 1 | -1): Promise<void> {
    const agents = connection.config?.agents ?? []
    const currentAgent = infoOf(id)?.agent
    if (agents.length === 0 || currentAgent === undefined) return
    const index = agents.findIndex((agent) => agent.name === currentAgent)
    const next = agents[(index + direction + agents.length) % agents.length]
    if (next) await setAgent(id, next.name)
  }

  async function compact(id: string): Promise<void> {
    await connection.getApi().compact(id)
  }

  // Returns the absolute path the backend wrote.
  async function exportSession(id: string, path: string): Promise<string> {
    return connection.getApi().exportTranscript(id, path)
  }

  // Sessions of one directory (or all), newest first. Includes the live ones.
  async function refreshHistory(forCwd?: string): Promise<void> {
    history.value = await connection.getApi().listHistory(forCwd, HISTORY_LIMIT)
    historyLoaded.value = true
  }

  function showInfo(info: SessionInfo): void {
    live.value = sortLive([...live.value.filter((session) => session.id !== info.id), info])
    chats.get(info.id)?.setInfo(info)
  }

  function setLive(sessions: readonly SessionInfo[]): void {
    const gone = live.value.some((old) => !sessions.some((session) => session.id === old.id))
    live.value = sortLive(sessions)
    for (const info of sessions) chats.get(info.id)?.setInfo(info)
    if (gone) void refreshHistory()
  }

  function onSessionEvent(frame: Extract<EventFrame, { kind: 'event' }>): void {
    const { session_id: id, event } = frame
    const session = chats.get(id)
    if (event.type === 'error' && id !== currentId.value) {
      notices.push('error', errorToast(session?.info?.name ?? id, event.text))
    }
    session?.applyEvent(event)
    if (event.type === 'closed') void discard(id, infoOf(id)?.cwd ?? cwd.value).catch(notices.pushError)
  }

  function onFrame(frame: EventFrame): void {
    if (frame.kind === 'sessions') setLive(frame.sessions)
    if (frame.kind === 'event') onSessionEvent(frame)
  }

  // Called when the connection opens. The first time, opens the requested session or a new one.
  async function onConnected(isReconnect: boolean, start: Startup): Promise<void> {
    const api = connection.getApi()
    if (isReconnect) {
      setLive(await api.listSessions())
      await Promise.all([...chats.values()].map((session) => session.resync()))
      return
    }
    cwd.value = start.cwd
    await connection.loadConfig()
    setLive(await api.listSessions())
    void refreshHistory().catch(notices.pushError)
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

  return {
    live,
    history,
    historyLoaded,
    currentId,
    current,
    focusedId,
    cwd,
    chat,
    openSession,
    newSession,
    closeSession,
    renameSession,
    deleteHistory,
    setSessionCwd,
    setModel,
    setAgent,
    setVariant,
    cycleAgent,
    compact,
    exportSession,
    refreshHistory,
    onFrame,
    onConnected,
  }
})
