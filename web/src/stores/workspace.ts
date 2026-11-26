// What the window shows: the live sessions, which one is open, and the actions on them.
//
//   frames --> Workspace --event--> ChatSession (one per session opened in this window)
//                 |--sessions--> live (the sidebar's list)
//
// A ChatSession stays while the session lives, so switching away only detaches it.

import { defineStore } from 'pinia'
import { computed, nextTick, ref, shallowRef, watch } from 'vue'
import { clamp } from '../clamp.ts'
import { crossfade } from '../composables/crossfade.ts'
import { emitDraftRestored } from '../composer/draftBus.ts'
import { ChatSession } from '../model/chatSession.ts'
import { collapseWhitespace } from '../model/oneLine.ts'
import type { EventFrame } from '../net/connection.ts'
import { writeLocation } from '../net/location.ts'
import { isWorkingSession } from '../shell/sessionRow.ts'
import { neighbor } from '../shell/tileLayout.ts'
import type { Rect } from '../shell/tileLayout.ts'
import type { Startup } from '../net/startup.ts'
import type { ModelRef, SessionInfo, StashGroup } from '../proto/generated'
import { useConnectionStore } from './connection.ts'
import { useNoticesStore } from './notices.ts'
import { useSettingsStore } from './settings.ts'
export type ViewMode = 'single' | 'split'
export type SplitFilter = string // 'all' | 'user' | <creator session id>

export interface NewSessionOptions {
  cwd?: string // default: the workspace's directory
  agent?: string
  model?: ModelRef
  name?: string
}

export interface HistoryFilterOptions {
  cwd?: string
  creator?: string
  query?: string
}
const HISTORY_LIMIT = 200
const ERROR_TOAST_MAX_CHARS = 200

// Live sessions in custom order first, then by creation time and id.
export function sortLive(sessions: readonly SessionInfo[], order: readonly string[] = []): SessionInfo[] {
  const rank = new Map<string, number>()
  for (const id of order) {
    if (!rank.has(id)) rank.set(id, rank.size)
  }
  return [...sessions].sort((a, b) => {
    const ia = rank.get(a.id)
    const ib = rank.get(b.id)
    if (ia !== undefined && ib !== undefined) return ia - ib
    if (ia !== undefined) return -1
    if (ib !== undefined) return 1
    return a.created_at - b.created_at || a.id.localeCompare(b.id)
  })
}

// "name: the error on one line", cut at 200 characters with an ellipsis (the TUI's toast for a
// session in the background: whitespace runs, newlines included, become one space).
export function errorToast(name: string, text: string): string {
  const oneLine = collapseWhitespace(text)
  const characters = Array.from(`${name}: ${oneLine}`)
  if (characters.length <= ERROR_TOAST_MAX_CHARS) return characters.join('')
  return `${characters.slice(0, ERROR_TOAST_MAX_CHARS - 1).join('')}…`
}

export const useWorkspaceStore = defineStore('workspace', () => {
  const connection = useConnectionStore()
  const notices = useNoticesStore()
  const settingsStore = useSettingsStore()
  const live = shallowRef<readonly SessionInfo[]>([])
  const history = shallowRef<readonly SessionInfo[]>([])
  const historyLoaded = ref(false)
  const sidebarHistory = shallowRef<readonly SessionInfo[]>([])
  const sidebarHistoryLoaded = ref(false)
  const sidebarHistorySynced = ref(false)
  const historyQuery = ref('')
  const historyCreator = ref<string>()
  const groups = shallowRef<readonly StashGroup[]>([])
  const splitFilter = ref<SplitFilter>('all')
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
  const detachingTiles = new Set<string>()
  let historySerial = 0
  let sidebarHistorySerial = 0
  let sidebarHistoryInFlight = 0
  let groupsSerial = 0
  let skipConfigWatch: { cwd: string | undefined } | undefined

  function skipNextConfigWatch(targetCwd: string | undefined): void {
    const token = { cwd: targetCwd }
    skipConfigWatch = token
    void nextTick().then(() => {
      if (skipConfigWatch === token) skipConfigWatch = undefined
    })
  }

  const current = computed(() => (currentId.value ? chat(currentId.value) : undefined))
  function isSplitTileEligible(session: SessionInfo): boolean {
    return session.live && session.id !== currentId.value && session.attached === 0
  }

  function matchesSplitFilter(session: SessionInfo, filter: SplitFilter): boolean {
    if (filter === 'user') return !session.parent
    if (filter !== 'all') return session.parent === filter
    return true
  }

  // Background sessions get a tile: live ones nobody is attached to, excluding the current foreground session.
  const allSplitTileIds = computed<readonly string[]>(() =>
    live.value.filter(isSplitTileEligible).map((session) => session.id),
  )
  const tileIds = computed(() => {
    const filter = splitFilter.value
    return live.value
      .filter((session) => isSplitTileEligible(session) && matchesSplitFilter(session, filter))
      .map((session) => session.id)
  })
  const visibleSessionIds = computed<string[]>(() => {
    if (mode.value === 'single') return currentId.value ? [currentId.value] : []
    return zoomedId.value ? [zoomedId.value] : tileIds.value
  })
  // The session commands act on when they are not given one: the focused tile in split view.
  const focusedId = computed(() =>
    mode.value === 'single' ? currentId.value : (zoomedId.value ?? tileFocusId.value),
  )
  const effectiveCwd = computed(
    () =>
      (focusedId.value ? infoOf(focusedId.value)?.cwd : undefined) ??
      current.value?.info?.cwd ??
      (cwd.value || undefined),
  )
  const historyCurrentDir = computed(() => settingsStore.settings.sidebarHistoryCurrentDir)
  watch(currentId, (id) => writeLocation({ sessionId: id }))
  watch(mode, (value) => writeLocation({ split: value === 'split' }))
  watch(tileIds, () => void syncTiles().catch(notices.pushError))
  watch(
    () => settingsStore.settings.sidebarRunningOrder,
    (order) => {
      live.value = sortLive(live.value, order)
    },
    { flush: 'sync' },
  )
  watch(
    () => settingsStore.settings.messageWindowSize,
    (size) => {
      for (const session of chats.values()) session.setWindowSize(size)
    },
  )
  watch(
    [
      historyQuery,
      historyCreator,
      () => settingsStore.settings.sidebarHistoryCreatorFilter,
      () => settingsStore.settings.sidebarHistoryCurrentDir,
    ],
    () => {
      sidebarHistorySynced.value = false
    },
    { flush: 'sync' },
  )
  watch(effectiveCwd, (nextCwd, prevCwd) => {
    const skipped = skipConfigWatch
    skipConfigWatch = undefined
    if (sidebarHistoryLoaded.value && settingsStore.settings.sidebarHistoryCurrentDir) {
      sidebarHistorySynced.value = false
      void refreshSidebarHistory().catch(notices.pushError)
    }
    if (skipped && nextCwd === skipped.cwd) return
    if (connection.state === 'open' && nextCwd !== prevCwd) {
      void connection.loadConfig(nextCwd, { silentWarnings: true }).catch(notices.pushError)
    }
  })
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
    created.setWindowSize(settingsStore.settings.messageWindowSize)
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
    const mine = ++latestOpen
    latestOpenTarget = id
    const run = openChain.then(() => (mode.value === 'split' ? openFromSplit(id, mine) : openOne(id, mine)))
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
    if (session.info?.live) showInfo(session.info)
    const previousId = currentId.value
    await crossfade(() => (currentId.value = id), 'standard')
    if (previousId === undefined || previousId === id) return
    await chat(previousId).detach()
  }

  async function openFromSplit(id: string, mine: number): Promise<void> {
    if (mine !== latestOpen) return
    const session = chat(id)
    if (session.attachment !== 'foreground') await session.setForeground(true)
    if (mine !== latestOpen) {
      const stillWanted = id === latestOpenTarget || id === currentId.value
      if (!stillWanted) await session.detach()
      return
    }
    if (session.info?.live) showInfo(session.info)
    const previousId = currentId.value
    currentId.value = id
    mode.value = 'single'
    zoomedId.value = undefined
    tileFocusId.value = undefined
    await connection.getApi().watchAll(false)
    const others = [...chats.values()].filter(
      (candidate) =>
        candidate !== session &&
        (candidate.attachment === 'watching' ||
          (candidate.id === previousId && candidate.attachment !== 'detached')),
    )
    await Promise.all(others.map((candidate) => candidate.detach()))
  }

  async function newSession(options: NewSessionOptions = {}): Promise<string> {
    const info = await connection.getApi().createSession(options.cwd ?? cwd.value, options)
    cwd.value = info.cwd
    live.value = sortLive(
      [...live.value.filter((session) => session.id !== info.id), info],
      settingsStore.settings.sidebarRunningOrder,
    )
    await openSession(info.id)
    return info.id
  }

  async function detachCurrent(): Promise<void> {
    latestOpen++
    latestOpenTarget = undefined
    if (mode.value === 'split') {
      if (zoomedId.value) unzoom()
      const previousId = currentId.value
      if (!previousId) return
      if (chat(previousId).attachment !== 'detached') await chat(previousId).detach()
      currentId.value = undefined
      await syncTiles()
      return
    }
    const previousId = currentId.value
    if (!previousId) return
    await crossfade(() => (currentId.value = undefined), 'standard')
    await chat(previousId).detach()
  }

  // A session is gone (closed here, by another client, or by the backend). When it was the open one in
  // single view, the oldest remaining live session takes over; when none is left, none is auto-created.
  // `listener` hears of every session that is gone (closed here or elsewhere), to drop what hangs on it.
  function onGone(listener: (id: string) => void): void {
    goneListeners.add(listener)
  }

  async function discard(id: string): Promise<void> {
    const known = chats.has(id) || live.value.some((session) => session.id === id)
    if (!known) return // already handled: the closed event and the response of Close both end here
    if (latestOpenTarget === id) {
      latestOpen++
      latestOpenTarget = undefined
    }
    chats.get(id)?.dispose()
    chats.delete(id)
    for (const listener of goneListeners) listener(id)
    live.value = live.value.filter((session) => session.id !== id)
    void refreshAllHistory().catch(notices.pushError)
    if (zoomedId.value === id) zoomedId.value = undefined
    if (currentId.value !== id) return
    currentId.value = undefined
    if (mode.value === 'split') return
    const next = live.value[0]
    if (next) await openSession(next.id)
  }

  async function closeSession(id: string): Promise<void> {
    await connection.getApi().close(id)
    await discard(id)
  }

  async function renameSession(id: string, name: string): Promise<void> {
    showInfo(await connection.getApi().rename(id, name))
  }

  async function deleteHistory(id: string): Promise<void> {
    await connection.getApi().deleteSession(id)
    history.value = history.value.filter((session) => session.id !== id)
    sidebarHistory.value = sidebarHistory.value.filter((session) => session.id !== id)
  }
  async function setSessionCwd(id: string, path: string): Promise<void> {
    const info = await connection.getApi().setCwd(id, path)
    const prevEffectiveCwd = effectiveCwd.value
    showInfo(info)
    cwd.value = info.cwd // new sessions of this window start there too
    if (effectiveCwd.value !== prevEffectiveCwd) skipNextConfigWatch(effectiveCwd.value)
    await connection.loadConfig(info.cwd)
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

  // Whether any session of the backend is running or waiting for the user.
  async function hasWorkingSessions(): Promise<boolean> {
    const sessions = await connection.getApi().listSessions()
    return sessions.some(isWorkingSession)
  }

  function peerCommand(sessionId: string, command: string, args?: string): Promise<string> {
    return connection.getApi().peerCommand(sessionId, command, args)
  }

  function listDirs(
    folder: string,
    options?: { cwd?: string; includeFiles?: boolean },
  ): Promise<{ path: string; dirs: string[]; files: string[] }> {
    return connection.getApi().listDirs(folder, options)
  }

  function currentSidebarHistoryFilter(): HistoryFilterOptions {
    const trimmedQuery = historyQuery.value.trim()
    const creator =
      historyCreator.value ||
      (settingsStore.settings.sidebarHistoryCreatorFilter === 'user' ? 'user' : undefined)
    const filterCwd = settingsStore.settings.sidebarHistoryCurrentDir ? effectiveCwd.value : undefined
    return {
      ...(filterCwd ? { cwd: filterCwd } : {}),
      ...(creator ? { creator } : {}),
      ...(trimmedQuery ? { query: trimmedQuery } : {}),
    }
  }

  function hasActiveSidebarFilter(): boolean {
    const active = currentSidebarHistoryFilter()
    return Boolean(active.cwd || active.creator || active.query)
  }

  function filterKey(options: HistoryFilterOptions): string {
    return JSON.stringify([options.cwd ?? '', options.creator ?? '', options.query ?? ''])
  }

  // Unfiltered sessions of one directory (or all), newest first. Includes the live ones.
  async function refreshHistory(forCwd?: string): Promise<void> {
    const serial = ++historySerial
    const result = await connection.getApi().listHistory({ cwd: forCwd, limit: HISTORY_LIMIT })
    if (serial !== historySerial) return
    history.value = result
    historyLoaded.value = true
    if (!forCwd && !hasActiveSidebarFilter() && sidebarHistoryInFlight === 0) {
      sidebarHistory.value = result
      sidebarHistoryLoaded.value = true
      sidebarHistorySynced.value = true
    }
  }

  // Sidebar history list, scoped by the active sidebar creator/directory/search filters.
  async function refreshSidebarHistory(options?: HistoryFilterOptions): Promise<void> {
    const serial = ++sidebarHistorySerial
    const active = options ?? currentSidebarHistoryFilter()
    const normalized: HistoryFilterOptions = {
      ...(active.cwd?.trim() ? { cwd: active.cwd.trim() } : {}),
      ...(active.creator?.trim() ? { creator: active.creator.trim() } : {}),
      ...(active.query?.trim() ? { query: active.query.trim() } : {}),
    }
    const startedWithKey = filterKey(normalized)
    sidebarHistoryInFlight++
    try {
      const result = await connection.getApi().listHistory({
        cwd: normalized.cwd,
        creator: normalized.creator,
        query: normalized.query,
        limit: HISTORY_LIMIT,
      })
      if (serial !== sidebarHistorySerial) return
      sidebarHistory.value = result
      sidebarHistoryLoaded.value = true
      if (options === undefined && filterKey(currentSidebarHistoryFilter()) === startedWithKey) {
        sidebarHistorySynced.value = true
      }
    } finally {
      sidebarHistoryInFlight--
    }
  }

  async function refreshAllHistory(): Promise<void> {
    if (!hasActiveSidebarFilter()) {
      await refreshHistory()
      return
    }
    await Promise.all([refreshHistory(), refreshSidebarHistory()])
  }
  // --- Split view

  // Watches the tiles' sessions and lets go of those that no longer have a tile (keeping the hidden foreground session).
  async function syncTiles(): Promise<void> {
    if (mode.value !== 'split') return
    const wanted = new Set(tileIds.value)
    const retained = new Set(allSplitTileIds.value)
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
      const isCurrent = id === currentId.value
      if (retained.has(id) || isCurrent || session.attachment === 'detached' || detachingTiles.has(id)) {
        continue
      }
      detachingTiles.add(id)
      jobs.push(session.detach().finally(() => detachingTiles.delete(id)))
    }
    if (tileFocusId.value !== undefined && !wanted.has(tileFocusId.value)) {
      tileFocusId.value = undefined
    }
    await Promise.all(jobs)
  }

  async function enterSplit(): Promise<void> {
    if (mode.value === 'split') return
    // Subscribing first, so that entering split view loses none of the background events.
    setLive(await connection.getApi().watchAll(true))
    // Not under a crossfade: mounting every tile's conversation inside a view transition holds
    // the page frozen for the snapshot. The tiles have their own entrance animation.
    mode.value = 'split'
    tileFocusId.value = undefined
    await syncTiles()
  }

  async function leaveSplit(): Promise<void> {
    if (mode.value === 'single') return
    if (zoomedId.value) currentId.value = zoomedId.value
    const targetId = currentId.value
    const mine = ++latestOpen
    latestOpenTarget = targetId
    mode.value = 'single'
    zoomedId.value = undefined
    tileFocusId.value = undefined
    const run = openChain.then(async () => {
      const open = targetId ? chat(targetId) : undefined
      await connection.getApi().watchAll(false)
      const others = [...chats.values()].filter(
        (session) => session !== open && session.attachment !== 'detached',
      )
      const attachOpen =
        open && mine === latestOpen && open.attachment !== 'foreground'
          ? open.setForeground(true)
          : Promise.resolve()
      await Promise.all([attachOpen, ...others.map((session) => session.detach())])
    })
    openChain = run.catch(() => undefined)
    return run
  }

  async function toggleSplit(): Promise<void> {
    if (mode.value === 'split') await leaveSplit()
    else await enterSplit()
  }

  // Enlarges a tile; it becomes the session that split view returns to.
  function zoomTile(id: string): void {
    if (mode.value !== 'split' || !tileIds.value.includes(id)) return
    tileFocusId.value = id
    zoomedId.value = id
  }

  function unzoom(): void {
    zoomedId.value = undefined
  }

  function focusTile(id: string | undefined): void {
    tileFocusId.value = id !== undefined && tileIds.value.includes(id) ? id : undefined
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
    live.value = sortLive(
      [...live.value.filter((session) => session.id !== info.id), info],
      settingsStore.settings.sidebarRunningOrder,
    )
    chats.get(info.id)?.setInfo(info)
  }

  function setLive(sessions: readonly SessionInfo[]): void {
    const gone = live.value.some((old) => !sessions.some((session) => session.id === old.id))
    const liveIds = new Set(sessions.map((session) => session.id))
    const currentOrder = settingsStore.settings.sidebarRunningOrder
    const pruned = currentOrder.filter((id) => liveIds.has(id))
    if (pruned.length !== currentOrder.length) {
      settingsStore.update((draft) => {
        draft.sidebarRunningOrder = pruned
      })
    }
    live.value = sortLive(sessions, pruned)
    for (const info of sessions) chats.get(info.id)?.setInfo(info)
    if (gone) void refreshAllHistory().catch(notices.pushError)
  }

  function moveRunningSession(id: string, toIndex: number): void {
    if (live.value.length <= 1) return
    const fromIndex = live.value.findIndex((session) => session.id === id)
    if (fromIndex < 0) return
    const clamped = clamp(toIndex, 0, live.value.length - 1)
    if (fromIndex === clamped) return
    const list = [...live.value]
    const [moved] = list.splice(fromIndex, 1)
    if (!moved) return
    list.splice(clamped, 0, moved)
    live.value = list
    const nextOrder = list.map((session) => session.id)
    settingsStore.update((draft) => {
      draft.sidebarRunningOrder = nextOrder
    })
  }
  function onSessionEvent(frame: Extract<EventFrame, { kind: 'event' }>): void {
    const { session_id: id, event } = frame
    const session = chats.get(id)
    const isInBackground = mode.value === 'split' || id !== currentId.value
    if (event.type === 'error' && isInBackground) {
      notices.push('error', errorToast(session?.info?.name ?? id, event.text))
    }
    session?.applyEvent(event)
    if (event.type === 'closed') void discard(id).catch(notices.pushError)
  }

  function onFrame(frame: EventFrame): void {
    if (frame.kind === 'sessions') setLive(frame.sessions)
    if (frame.kind === 'event') onSessionEvent(frame)
    if (frame.kind === 'groups_changed') groups.value = frame.groups
  }

  function setSplitFilter(filter: SplitFilter): void {
    splitFilter.value = filter
  }

  async function refreshGroups(): Promise<void> {
    const api = connection.getApi()
    const serial = ++groupsSerial
    const loaded = await api.listGroups()
    if (serial !== groupsSerial) return
    groups.value = loaded
  }

  async function syncSessionsAfterStashChange(): Promise<void> {
    const api = connection.getApi()
    const sessions = mode.value === 'split' ? await api.watchAll(true) : await api.listSessions()
    setLive(sessions)
    void refreshAllHistory().catch(notices.pushError)
  }

  async function stashAll(name?: string): Promise<StashGroup | null> {
    const result = await connection.getApi().stashAll(name)
    groupsSerial++
    groups.value = result.groups
    await syncSessionsAfterStashChange()
    return result.group
  }

  async function createGroup(
    sessionIds: string[],
    options: { name?: string; stashLive?: boolean } = {},
  ): Promise<StashGroup> {
    const result = await connection.getApi().createGroup(sessionIds, options)
    groupsSerial++
    groups.value = result.groups
    await syncSessionsAfterStashChange()
    return result.group
  }

  async function renameGroup(groupId: string, name: string): Promise<void> {
    const next = await connection.getApi().renameGroup(groupId, name)
    groupsSerial++
    groups.value = next
  }

  async function setGroupLocked(groupId: string, locked: boolean): Promise<void> {
    const next = await connection.getApi().setGroupLocked(groupId, locked)
    groupsSerial++
    groups.value = next
  }

  async function deleteGroup(groupId: string): Promise<void> {
    const next = await connection.getApi().deleteGroup(groupId)
    groupsSerial++
    groups.value = next
  }

  async function addToGroup(
    groupId: string,
    sessionId: string,
    options: { position?: number; stashLive?: boolean } = {},
  ): Promise<void> {
    const next = await connection.getApi().addToGroup(groupId, sessionId, options)
    groupsSerial++
    groups.value = next
    if (options.stashLive ?? true) await syncSessionsAfterStashChange()
  }

  async function moveGroupMember(
    toGroupId: string,
    sessionId: string,
    position: number,
    options: { fromGroupId?: string; stashLive?: boolean } = {},
  ): Promise<void> {
    const next = await connection.getApi().moveGroupMember(toGroupId, sessionId, position, options)
    groupsSerial++
    groups.value = next
    if ((options.stashLive ?? true) && !options.fromGroupId) await syncSessionsAfterStashChange()
  }

  async function removeFromGroup(groupId: string, sessionId: string): Promise<void> {
    const next = await connection.getApi().removeFromGroup(groupId, sessionId)
    groupsSerial++
    groups.value = next
  }

  async function mergeGroups(sourceGroupId: string, targetGroupId: string): Promise<void> {
    const next = await connection.getApi().mergeGroups(sourceGroupId, targetGroupId)
    groupsSerial++
    groups.value = next
  }

  async function restoreGroup(groupId: string): Promise<string[]> {
    const result = await connection.getApi().restoreGroup(groupId)
    groupsSerial++
    groups.value = result.groups
    await syncSessionsAfterStashChange()
    return result.restored_ids
  }

  async function restoreGroupMember(groupId: string, sessionId: string): Promise<boolean> {
    const result = await connection.getApi().restoreGroupMember(groupId, sessionId)
    groupsSerial++
    groups.value = result.groups
    await syncSessionsAfterStashChange()
    return result.restored
  }

  function resetForBackendSwitch(): void {
    latestOpen++
    latestOpenTarget = undefined
    openChain = Promise.resolve()
    historySerial++
    sidebarHistorySerial++
    groupsSerial++
    skipConfigWatch = undefined
    notices.clear()
    for (const session of chats.values()) session.dispose()
    chats.clear()
    attachingTiles.clear()
    detachingTiles.clear()
    live.value = []
    history.value = []
    historyLoaded.value = false
    sidebarHistory.value = []
    sidebarHistoryLoaded.value = false
    sidebarHistorySynced.value = false
    groups.value = []
    currentId.value = undefined
    zoomedId.value = undefined
    tileFocusId.value = undefined
    tileRects.value = []
    cwd.value = ''
    splitFilter.value = 'all'
  }

  // Called when the connection opens. The first time, opens the requested session if given.
  async function onConnected(isReconnect: boolean, start: Startup): Promise<void> {
    const api = connection.getApi()
    if (isReconnect) {
      await connection.loadConfig(effectiveCwd.value)
      const sessions = mode.value === 'split' ? await api.watchAll(true) : await api.listSessions()
      setLive(sessions)
      // A session closed while the connection was down never sent its `closed` event.
      for (const id of [...chats.keys()]) {
        if (sessions.some((session) => session.id === id)) continue
        await discard(id)
      }
      await Promise.all([...chats.values()].map((session) => session.resync()))
      void refreshGroups().catch(notices.pushError)
      return
    }
    const prevEffectiveCwd = effectiveCwd.value
    cwd.value = start.cwd ?? connection.hello?.cwd ?? ''
    if (effectiveCwd.value !== prevEffectiveCwd) skipNextConfigWatch(effectiveCwd.value)
    await connection.loadConfig(effectiveCwd.value)
    setLive(await api.listSessions())
    void refreshAllHistory().catch(notices.pushError)
    void refreshGroups().catch(notices.pushError)
    await openRequested(start.sessionId)
    if (start.split) await enterSplit()
  }

  async function openRequested(sessionId: string | undefined): Promise<void> {
    if (!sessionId) return
    try {
      await openSession(sessionId)
    } catch (error) {
      notices.pushError(error)
    }
  }

  return {
    live,
    history,
    historyLoaded,
    sidebarHistory,
    sidebarHistoryLoaded,
    sidebarHistorySynced,
    historyQuery,
    historyCreator,
    historyCurrentDir,
    groups,
    splitFilter,
    currentId,
    current,
    focusedId,
    mode,
    zoomedId,
    tileFocusId,
    allSplitTileIds,
    tileIds,
    visibleSessionIds,
    cwd,
    effectiveCwd,
    chat,
    infoOf,
    onGone,
    openSession,
    newSession,
    detachCurrent,
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
    hasWorkingSessions,
    peerCommand,
    listDirs,
    refreshHistory,
    refreshSidebarHistory,
    moveRunningSession,
    enterSplit,
    leaveSplit,
    toggleSplit,
    zoomTile,
    unzoom,
    focusTile,
    moveTileFocus,
    setTileRects,
    setSplitFilter,
    refreshGroups,
    stashAll,
    createGroup,
    renameGroup,
    setGroupLocked,
    deleteGroup,
    addToGroup,
    moveGroupMember,
    removeFromGroup,
    mergeGroups,
    restoreGroup,
    restoreGroupMember,
    resetForBackendSwitch,
    onFrame,
    onConnected,
  }
})
