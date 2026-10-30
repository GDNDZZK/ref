// What the window shows: the live sessions, which one is open, and the actions on them.
//
//   frames --> Workspace --event--> ChatSession (one per session opened in this window)
//                 |--sessions--> live (the sidebar's list)
//
// A ChatSession stays while the session lives, so switching away only detaches it.

import { defineStore } from 'pinia'
import { computed, ref, shallowRef, watch } from 'vue'
import { crossfade } from '../composables/crossfade.ts'
import { ChatSession } from '../model/chatSession.ts'
import type { EventFrame } from '../net/connection.ts'
import { writeLocation } from '../net/location.ts'
import { neighbor } from '../shell/tileLayout.ts'
import type { Rect } from '../shell/tileLayout.ts'
import type { Startup } from '../net/startup.ts'
import type { ModelRef, SessionInfo } from '../proto/generated'
import { useConnectionStore } from './connection.ts'
import { emitDraftRestored } from './draftBus.ts'
import { useNoticesStore } from './notices.ts'

export type ViewMode = 'single' | 'split'

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
  const currentId = ref<string>() // in split view: the session that returns when split view is left
  const cwd = ref('') // where new sessions start
  const mode = ref<ViewMode>('single')
  const zoomedId = ref<string>() // split view: the tile enlarged to the whole content area
  const tileFocusId = ref<string>()
  const tileRects = shallowRef<readonly Rect[]>([]) // where the split view drew the tiles, for moving the focus

  const chats = new Map<string, ChatSession>()
  let openChain: Promise<void> = Promise.resolve()
  let latestOpen = 0
  let latestOpenTarget: string | undefined
  const goneListeners = new Set<(id: string) => void>()
  const attachingTiles = new Set<string>() // tiles whose first Attach has not been answered yet

  const current = computed(() => (currentId.value ? chat(currentId.value) : undefined))
  // Background sessions get a tile: live ones nobody is attached to (the TUI's rule), and the open one,
  // which is backgrounded while split view is on.
  const tileIds = computed(() =>
    live.value
      .filter((session) => session.live && (session.attached === 0 || session.id === currentId.value))
      .map((session) => session.id),
  )
  // What the window shows now: one session, the zoomed tile, or all tiles.
  const visibleSessionIds = computed<string[]>(() => {
    if (mode.value === 'single') return currentId.value ? [currentId.value] : []
    return zoomedId.value ? [zoomedId.value] : tileIds.value
  })
  // The session commands act on when they are not given one: the focused tile in split view.
  const focusedId = computed(() =>
    mode.value === 'single' ? currentId.value : (zoomedId.value ?? tileFocusId.value),
  )

  watch(currentId, (id) => writeLocation({ sessionId: id }))
  watch(mode, (value) => writeLocation({ split: value === 'split' }))
  watch(tileIds, () => void syncTiles().catch(notices.pushError))

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
        draftRestored: emitDraftRestored,
      },
      info,
    )
    chats.set(id, created)
    return created
  }

  function infoOf(id: string): SessionInfo | undefined {
    return chats.get(id)?.info ?? live.value.find((session) => session.id === id)
  }

  // Makes `id` the open session, attached in the foreground. The one shown before is detached.
  // Opens run one after the other. Of several quick clicks only the last one counts: the earlier
  // ones either do not start or undo their own attach, so no session stays attached by mistake.
  function openSession(id: string): Promise<void> {
    if (mode.value === 'split') {
      currentId.value = id
      return leaveSplit()
    }
    const mine = ++latestOpen
    latestOpenTarget = id
    const run = openChain.then(() => openOne(id, mine))
    openChain = run.catch(() => undefined)
    return run
  }

  async function openOne(id: string, mine: number): Promise<void> {
    if (mine !== latestOpen) return // a later click replaced this one before it began
    const session = chat(id)
    await session.attach(true)
    if (mine !== latestOpen) {
      const stillWanted = id === latestOpenTarget || id === currentId.value
      if (!stillWanted) await session.detach()
      return
    }
    const previousId = currentId.value
    await crossfade(() => (currentId.value = id), 'standard')
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
  // `listener` hears of every session that is gone (closed here or elsewhere), to drop what hangs on it.
  function onGone(listener: (id: string) => void): void {
    goneListeners.add(listener)
  }

  async function discard(id: string, lastCwd: string): Promise<void> {
    const known = chats.has(id) || live.value.some((session) => session.id === id)
    if (!known) return // already handled: the closed event and the response of Close both end here
    chats.get(id)?.dispose()
    chats.delete(id)
    for (const listener of goneListeners) listener(id)
    live.value = live.value.filter((session) => session.id !== id)
    void refreshHistory()
    if (zoomedId.value === id) zoomedId.value = undefined
    if (currentId.value !== id) return
    currentId.value = undefined
    const next = live.value[0]
    if (next && mode.value === 'split') {
      currentId.value = next.id
    } else if (next) {
      await openSession(next.id)
    } else {
      mode.value = 'single'
      await newSession({ cwd: lastCwd })
    }
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

  // Back to the model's default (no variant).
  async function clearVariant(id: string): Promise<void> {
    showInfo(await connection.getApi().setVariant(id, ''))
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

  // --- Split view

  // Watches the tiles' sessions and lets go of those that no longer have a tile.
  async function syncTiles(): Promise<void> {
    if (mode.value !== 'split') return
    const wanted = new Set(tileIds.value)
    const jobs: Promise<void>[] = []
    for (const id of wanted) {
      if (chat(id).attachment !== 'detached' || attachingTiles.has(id)) continue
      attachingTiles.add(id)
      jobs.push(
        chat(id)
          .attach(false)
          .finally(() => attachingTiles.delete(id)),
      )
    }
    for (const [id, session] of chats) {
      if (!wanted.has(id) && session.attachment === 'watching') jobs.push(session.detach())
    }
    if (tileFocusId.value === undefined || !wanted.has(tileFocusId.value)) {
      tileFocusId.value = currentId.value && wanted.has(currentId.value) ? currentId.value : tileIds.value[0]
    }
    await Promise.all(jobs)
  }

  async function enterSplit(): Promise<void> {
    if (mode.value === 'split') return
    // Subscribing first, so that backgrounding the open session loses none of its events.
    setLive(await connection.getApi().watchAll(true))
    mode.value = 'split'
    tileFocusId.value = currentId.value
    const open = current.value
    await Promise.all([open?.setForeground(false), syncTiles()])
  }

  async function leaveSplit(): Promise<void> {
    if (mode.value === 'single') return
    const open = current.value
    mode.value = 'single'
    zoomedId.value = undefined
    await connection.getApi().watchAll(false)
    const others = [...chats.values()].filter(
      (session) => session !== open && session.attachment === 'watching',
    )
    await Promise.all([open?.setForeground(true), ...others.map((session) => session.detach())])
  }

  async function toggleSplit(): Promise<void> {
    if (mode.value === 'split') await leaveSplit()
    else await enterSplit()
  }

  // Enlarges a tile; it becomes the session that split view returns to.
  function zoomTile(id: string): void {
    if (mode.value !== 'split') return
    currentId.value = id
    tileFocusId.value = id
    zoomedId.value = id
  }

  function unzoom(): void {
    zoomedId.value = undefined
  }

  function focusTile(id: string): void {
    if (tileIds.value.includes(id)) tileFocusId.value = id
  }

  // The tile nearest to the focused one in the direction (dx, dy), each -1, 0 or 1.
  function moveTileFocus(dx: number, dy: number): void {
    const ids = tileIds.value
    const from = tileFocusId.value ? ids.indexOf(tileFocusId.value) : -1
    const target = from < 0 ? 0 : neighbor(tileRects.value, from, dx, dy)
    const id = target === undefined ? undefined : ids[target]
    if (id) tileFocusId.value = id
  }

  function setTileRects(rects: readonly Rect[]): void {
    tileRects.value = rects
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
    if (event.type === 'error' && (mode.value === 'split' || id !== currentId.value)) {
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
      setLive(mode.value === 'split' ? await api.watchAll(true) : await api.listSessions())
      await Promise.all([...chats.values()].map((session) => session.resync()))
      return
    }
    cwd.value = start.cwd
    await connection.loadConfig()
    setLive(await api.listSessions())
    void refreshHistory().catch(notices.pushError)
    await openRequested(start.sessionId)
    if (start.split) await enterSplit()
  }

  async function openRequested(sessionId: string | undefined): Promise<void> {
    if (sessionId) {
      try {
        await openSession(sessionId)
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
    mode,
    zoomedId,
    tileFocusId,
    tileIds,
    visibleSessionIds,
    cwd,
    chat,
    infoOf,
    onGone,
    openSession,
    newSession,
    closeSession,
    renameSession,
    deleteHistory,
    setSessionCwd,
    setModel,
    setAgent,
    setVariant,
    clearVariant,
    cycleAgent,
    compact,
    exportSession,
    refreshHistory,
    enterSplit,
    leaveSplit,
    toggleSplit,
    zoomTile,
    unzoom,
    focusTile,
    moveTileFocus,
    setTileRects,
    onFrame,
    onConnected,
  }
})
