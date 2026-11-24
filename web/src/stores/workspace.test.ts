import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { info, snapshot, userMessage } from '../model/testMessages.ts'
import type { SessionInfo } from '../proto/generated'
import { STORAGE_KEY } from '../settings/model.ts'
import { useNoticesStore } from './notices.ts'
import { useSettingsStore } from './settings.ts'
import { errorToast, sortLive, useWorkspaceStore } from './workspace.ts'
const api = {
  attach: vi.fn(),
  detach: vi.fn(),
  close: vi.fn(),
  createSession: vi.fn(),
  listSessions: vi.fn(),
  listHistory: vi.fn(),
  watchAll: vi.fn(),
  setAgent: vi.fn(),
  setCwd: vi.fn(),
  listGroups: vi.fn(),
}
const connection = {
  getApi: () => api,
  hello: { cwd: '/server' } as { cwd?: string } | undefined,
  config: undefined as unknown,
  loadConfig: vi.fn(),
}

vi.mock('./connection.ts', () => ({ useConnectionStore: () => connection }))

const session = (id: string, overrides: Partial<SessionInfo> = {}) =>
  info({ id, name: id, created_at: Number(id.replace(/\D/g, '')), ...overrides })

function setup(sessions: SessionInfo[]) {
  const workspace = useWorkspaceStore()
  workspace.onFrame({ kind: 'sessions', sessions })
  api.watchAll.mockImplementation(() => Promise.resolve(sessions)) // the backend answers with the live sessions
  api.attach.mockImplementation((id: string) => {
    const known = sessions.find((entry) => entry.id === id) ?? session(id)
    return Promise.resolve(snapshot([userMessage('u1', 'hi')], { info: known }))
  })
  return workspace
}

beforeEach(() => {
  localStorage.clear()
  setActivePinia(createPinia())
  for (const fn of Object.values(api)) fn.mockReset().mockResolvedValue(undefined)
  api.listSessions.mockResolvedValue([])
  api.listHistory.mockResolvedValue([])
  api.listGroups.mockResolvedValue([])
  connection.config = { agents: [{ name: 'build' }, { name: 'plan' }, { name: 'review' }] }
})

describe('sortLive', () => {
  it('orders by creation time, then id', () => {
    const sorted = sortLive([session('ses_3'), session('ses_1'), session('b', { created_at: 3 })])
    expect(sorted.map((entry) => entry.id)).toEqual(['ses_1', 'b', 'ses_3'])
  })

  it('places ordered sessions first using first occurrence, then falls back for new ones', () => {
    const sorted = sortLive(
      [session('ses_1'), session('ses_2'), session('ses_3'), session('ses_4')],
      ['ses_3', 'ses_1', 'ses_3'],
    )
    expect(sorted.map((entry) => entry.id)).toEqual(['ses_3', 'ses_1', 'ses_2', 'ses_4'])
  })
})

describe('running session order', () => {
  it('reorders sessions, persists to localStorage, and keeps order across updates', () => {
    const workspace = setup([session('ses_1'), session('ses_2'), session('ses_3')])
    workspace.moveRunningSession('ses_3', 0)
    expect(workspace.live.map((entry) => entry.id)).toEqual(['ses_3', 'ses_1', 'ses_2'])
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({
      sidebarRunningOrder: ['ses_3', 'ses_1', 'ses_2'],
    })

    workspace.onFrame({
      kind: 'sessions',
      sessions: [session('ses_1'), session('ses_2'), session('ses_3'), session('ses_4')],
    })
    expect(workspace.live.map((entry) => entry.id)).toEqual(['ses_3', 'ses_1', 'ses_2', 'ses_4'])
  })

  it('prunes closed sessions from sidebarRunningOrder only when needed', () => {
    const workspace = setup([session('ses_1'), session('ses_2'), session('ses_3')])
    workspace.moveRunningSession('ses_3', 0)
    const updateSpy = vi.spyOn(useSettingsStore(), 'update')
    workspace.onFrame({ kind: 'sessions', sessions: [session('ses_1'), session('ses_2'), session('ses_3')] })
    expect(updateSpy).not.toHaveBeenCalled()

    workspace.onFrame({ kind: 'sessions', sessions: [session('ses_1'), session('ses_3')] })
    expect(updateSpy).toHaveBeenCalledTimes(1)
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toMatchObject({
      sidebarRunningOrder: ['ses_3', 'ses_1'],
    })
    updateSpy.mockRestore()
  })
  it('re-sorts live sessions when another tab updates sidebarRunningOrder', () => {
    const workspace = setup([session('ses_1'), session('ses_2'), session('ses_3')])
    window.dispatchEvent(
      new StorageEvent('storage', {
        key: STORAGE_KEY,
        newValue: JSON.stringify({ sidebarRunningOrder: ['ses_2', 'ses_3', 'ses_1'] }),
      }),
    )
    expect(workspace.live.map((entry) => entry.id)).toEqual(['ses_2', 'ses_3', 'ses_1'])
  })
})
describe('errorToast', () => {
  it('names the session and puts the whole error on one line, as the TUI does', () => {
    expect(errorToast('parser', 'rate limited\n\n  retry\tlater \n')).toBe('parser: rate limited retry later')
  })

  it('cuts long text at 200 characters', () => {
    const toast = errorToast('x', 'e'.repeat(500))
    expect(toast).toHaveLength(200)
    expect(toast).toBe(`x: ${'e'.repeat(196)}…`)
  })

  it('counts characters, not UTF-16 units', () => {
    const toast = errorToast('x', '😀'.repeat(500))
    expect(Array.from(toast)).toHaveLength(200)
    expect(toast.endsWith('😀…')).toBe(true)
  })
})

describe('opening sessions', () => {
  it('attaches in the foreground and detaches the one shown before', async () => {
    const workspace = setup([session('ses_1'), session('ses_2')])
    await workspace.openSession('ses_1')
    await workspace.openSession('ses_2')
    expect(api.attach).toHaveBeenLastCalledWith('ses_2', true)
    expect(api.detach).toHaveBeenCalledWith('ses_1')
    expect(workspace.currentId).toBe('ses_2')
    expect(workspace.visibleSessionIds).toEqual(['ses_2'])
  })

  // The backend counts the attached clients: a session left attached keeps counting this window.
  it.each([
    ['the first answer comes first', 5, 20],
    ['the second answer comes first', 20, 5],
  ])('leaves only the last of two quick clicks attached when %s', async (_name, slowMs, fastMs) => {
    const sessions = [session('ses_1'), session('ses_2'), session('ses_3')]
    const workspace = setup(sessions)
    const attached = new Set<string>()
    const delays: Record<string, number> = { ses_1: 0, ses_2: slowMs, ses_3: fastMs }
    api.attach.mockImplementation(
      (id: string) =>
        new Promise((resolve) =>
          setTimeout(() => {
            attached.add(id)
            resolve(snapshot([userMessage('u1', 'hi')], { info: sessions.find((entry) => entry.id === id)! }))
          }, delays[id]),
        ),
    )
    api.detach.mockImplementation((id: string) => {
      attached.delete(id)
      return Promise.resolve()
    })
    await workspace.openSession('ses_1')

    await Promise.all([workspace.openSession('ses_2'), workspace.openSession('ses_3')])

    expect(workspace.currentId).toBe('ses_3')
    expect([...attached]).toEqual(['ses_3'])
  })

  it('keeps the open session when attaching fails', async () => {
    const workspace = setup([session('ses_1')])
    await workspace.openSession('ses_1')
    api.attach.mockRejectedValueOnce(new Error('gone'))
    await expect(workspace.openSession('ses_9')).rejects.toThrow('gone')
    expect(workspace.currentId).toBe('ses_1')
  })

  it('makes a new session, updates workspace cwd, and opens it', async () => {
    const workspace = setup([])
    workspace.cwd = '/work'
    api.createSession.mockResolvedValue(session('ses_5', { cwd: '/chosen/dir' }))
    expect(await workspace.newSession({ cwd: '/chosen/dir' })).toBe('ses_5')
    expect(api.createSession).toHaveBeenCalledWith('/chosen/dir', { cwd: '/chosen/dir' })
    expect(workspace.cwd).toBe('/chosen/dir')
    expect(workspace.currentId).toBe('ses_5')
    expect(workspace.live.map((entry) => entry.id)).toEqual(['ses_5'])
  })
})

describe('startup onConnected', () => {
  it('does not auto-create a session when sessionId is undefined', async () => {
    const workspace = useWorkspaceStore()
    api.listSessions.mockResolvedValue([session('ses_1', { attached: 0 })])
    await workspace.onConnected(false, {
      url: 'ws://x/ws',
      cwd: '/home/user',
      sessionId: undefined,
      split: false,
    })
    expect(api.createSession).not.toHaveBeenCalled()
    expect(api.attach).not.toHaveBeenCalled()
    expect(workspace.currentId).toBeUndefined()
    expect(workspace.cwd).toBe('/home/user')
    expect(workspace.mode).toBe('single')
  })

  it('enters split view without auto-creating a session when split is true and sessionId is undefined', async () => {
    const s1 = session('ses_1', { attached: 0 })
    const workspace = setup([s1])
    api.listSessions.mockResolvedValue([s1])
    await workspace.onConnected(false, { url: 'ws://x/ws', cwd: '/work', sessionId: undefined, split: true })
    expect(api.createSession).not.toHaveBeenCalled()
    expect(workspace.currentId).toBeUndefined()
    expect(workspace.mode).toBe('split')
    expect(workspace.tileIds).toEqual(['ses_1'])
  })

  it('pushes an error notice and leaves currentId undefined when requested sessionId fails to open', async () => {
    const workspace = useWorkspaceStore()
    api.listSessions.mockResolvedValue([])
    api.attach.mockRejectedValueOnce(new Error('session missing'))
    await workspace.onConnected(false, {
      url: 'ws://x/ws',
      cwd: '/work',
      sessionId: 'ses_dead',
      split: false,
    })
    expect(api.createSession).not.toHaveBeenCalled()
    expect(workspace.currentId).toBeUndefined()
    expect(useNoticesStore().notices.map((n) => n.text)).toEqual(['session missing'])
  })
})

describe('reconnecting', () => {
  const startup = { url: 'ws://x/ws', cwd: '/work', sessionId: undefined, split: false }

  it('moves on from a session that was closed while the connection was down', async () => {
    const workspace = setup([session('ses_1'), session('ses_2')])
    await workspace.openSession('ses_1')
    api.listSessions.mockResolvedValue([session('ses_2')])

    await workspace.onConnected(true, startup)

    expect(connection.loadConfig).toHaveBeenCalled()
    expect(workspace.live.map((entry) => entry.id)).toEqual(['ses_2'])
    expect(workspace.currentId).toBe('ses_2')
  })

  it('reloads config when setSessionCwd changes the working directory', async () => {
    const workspace = setup([session('ses_1', { cwd: '/old' })])
    connection.loadConfig.mockClear()
    api.setCwd.mockResolvedValue(session('ses_1', { cwd: '/new' }))
    await workspace.setSessionCwd('ses_1', '/new')
    expect(workspace.cwd).toBe('/new')
    expect(connection.loadConfig).toHaveBeenCalledTimes(1)
  })
})

describe('closing sessions', () => {
  it('goes to the oldest live session when the open one is closed', async () => {
    const workspace = setup([session('ses_1'), session('ses_2'), session('ses_3')])
    await workspace.openSession('ses_3')
    await workspace.closeSession('ses_3')
    expect(workspace.currentId).toBe('ses_1')
    expect(workspace.live.map((entry) => entry.id)).toEqual(['ses_1', 'ses_2'])
  })

  it('leaves currentId undefined in single mode without creating a session when the last one is closed', async () => {
    const workspace = setup([session('ses_1', { cwd: '/proj' })])
    await workspace.openSession('ses_1')
    await workspace.closeSession('ses_1')
    expect(api.createSession).not.toHaveBeenCalled()
    expect(workspace.currentId).toBeUndefined()
    expect(workspace.mode).toBe('single')
  })

  it('clears currentId in split mode without stealing the next background tile or leaving split mode', async () => {
    const workspace = setup([session('ses_1'), session('ses_2', { attached: 0 })])
    await workspace.openSession('ses_1')
    await workspace.enterSplit()
    workspace.onFrame({
      kind: 'sessions',
      sessions: [session('ses_1', { attached: 0 }), session('ses_2', { attached: 0 })],
    })
    expect(workspace.tileIds).toEqual(['ses_2'])

    await workspace.closeSession('ses_1')
    expect(workspace.mode).toBe('split')
    expect(workspace.currentId).toBeUndefined()
    expect(workspace.tileIds).toEqual(['ses_2'])
    expect(api.createSession).not.toHaveBeenCalled()

    await workspace.closeSession('ses_2')
    expect(workspace.mode).toBe('split')
    expect(workspace.currentId).toBeUndefined()
    expect(workspace.tileIds).toEqual([])
    expect(api.createSession).not.toHaveBeenCalled()
  })

  it('does not act twice when the closed event comes before the response', async () => {
    const workspace = setup([session('ses_1'), session('ses_2')])
    await workspace.openSession('ses_1')
    api.attach.mockClear()
    api.close.mockImplementation(() => {
      workspace.onFrame({ kind: 'event', session_id: 'ses_1', event: { type: 'closed' } })
      return Promise.resolve()
    })
    await workspace.closeSession('ses_1')
    await vi.waitFor(() => expect(workspace.currentId).toBe('ses_2'))
    expect(api.attach).toHaveBeenCalledTimes(1)
  })
})

describe('detachCurrent', () => {
  it('detaches the foreground session in single mode and clears currentId', async () => {
    const workspace = setup([session('ses_1')])
    await workspace.openSession('ses_1')
    api.detach.mockClear()
    await workspace.detachCurrent()
    expect(workspace.currentId).toBeUndefined()
    expect(workspace.current).toBeUndefined()
    expect(workspace.visibleSessionIds).toEqual([])
    expect(api.detach).toHaveBeenCalledWith('ses_1')
  })

  it('cancels an in-flight openSession so it detaches immediately upon settling', async () => {
    const workspace = setup([session('ses_1')])
    let resolveAttach!: () => void
    api.attach.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveAttach = () => resolve(snapshot([userMessage('u1', 'hi')], { info: session('ses_1') }))
        }),
    )
    const opening = workspace.openSession('ses_1')
    await Promise.resolve() // let openChain begin attach
    await workspace.detachCurrent()
    resolveAttach()
    await opening
    expect(workspace.currentId).toBeUndefined()
    expect(api.detach).toHaveBeenCalledWith('ses_1')
  })

  it('unzooms and returns the hidden foreground session to background tiles in split mode', async () => {
    const workspace = setup([session('ses_1'), session('ses_2', { attached: 0 })])
    await workspace.openSession('ses_1')
    await workspace.enterSplit()
    workspace.onFrame({
      kind: 'sessions',
      sessions: [session('ses_1', { attached: 0 }), session('ses_2', { attached: 0 })],
    })
    workspace.zoomTile('ses_2')
    api.attach.mockClear()

    await workspace.detachCurrent()
    expect(workspace.zoomedId).toBeUndefined()
    expect(workspace.currentId).toBeUndefined()
    expect(workspace.tileIds).toEqual(['ses_1', 'ses_2'])
    expect(api.attach).toHaveBeenCalledWith('ses_1', false)
  })
})

describe('the history list', () => {
  it('tells the user when it cannot be refreshed after a session is closed', async () => {
    const workspace = setup([session('ses_1'), session('ses_2')])
    await workspace.openSession('ses_1')
    api.listHistory.mockRejectedValue(new Error('history unavailable'))
    await workspace.closeSession('ses_2')
    await vi.waitFor(() =>
      expect(useNoticesStore().notices.map((notice) => notice.text)).toEqual(['history unavailable']),
    )
  })

  it('keeps workspace.history unfiltered while refreshSidebarHistory forwards filters and drops stale responses', async () => {
    const workspace = setup([])
    api.listHistory.mockResolvedValueOnce([session('ses_all_1'), session('ses_all_2', { cwd: '/other' })])
    await workspace.refreshHistory()
    expect(workspace.history.map((entry) => entry.id)).toEqual(['ses_all_1', 'ses_all_2'])

    let resolveSlow!: (value: SessionInfo[]) => void
    let resolveFast!: (value: SessionInfo[]) => void
    api.listHistory
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveSlow = resolve
          }),
      )
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFast = resolve
          }),
      )

    const slow = workspace.refreshSidebarHistory({ cwd: '/a', creator: 'user', query: 'first' })
    const fast = workspace.refreshSidebarHistory({ cwd: '/b', creator: 'ses_lead', query: 'second' })
    expect(api.listHistory).toHaveBeenNthCalledWith(2, {
      cwd: '/a',
      creator: 'user',
      query: 'first',
      limit: 200,
    })
    expect(api.listHistory).toHaveBeenNthCalledWith(3, {
      cwd: '/b',
      creator: 'ses_lead',
      query: 'second',
      limit: 200,
    })

    resolveFast([session('ses_2')])
    await fast
    expect(workspace.sidebarHistory.map((entry) => entry.id)).toEqual(['ses_2'])
    expect(workspace.sidebarHistoryLoaded).toBe(true)
    // Unfiltered workspace.history stays untouched so SessionsDialog and /session always see everything.
    expect(workspace.history.map((entry) => entry.id)).toEqual(['ses_all_1', 'ses_all_2'])

    resolveSlow([session('ses_1')])
    await slow
    expect(workspace.sidebarHistory.map((entry) => entry.id)).toEqual(['ses_2'])
    expect(workspace.history.map((entry) => entry.id)).toEqual(['ses_all_1', 'ses_all_2'])
  })

  it('preserves active sidebar filters across background refreshes while keeping history unfiltered', async () => {
    const workspace = setup([session('ses_1', { cwd: '/proj' }), session('ses_2', { cwd: '/proj' })])
    await workspace.openSession('ses_1')
    const settings = useSettingsStore()
    settings.update((draft) => {
      draft.sidebarHistoryCreatorFilter = 'user'
      draft.sidebarHistoryCurrentDir = true
    })
    workspace.historyQuery = 'alpha'
    workspace.historyCreator = 'ses_lead'
    api.listHistory.mockClear()
    api.listHistory.mockImplementation((options?: { query?: string }) =>
      Promise.resolve(
        options?.query ? [session('ses_filtered')] : [session('ses_all'), session('ses_filtered')],
      ),
    )

    await workspace.closeSession('ses_2')
    await vi.waitFor(() => {
      expect(api.listHistory).toHaveBeenCalledWith({ cwd: undefined, limit: 200 })
      expect(api.listHistory).toHaveBeenCalledWith({
        cwd: '/proj',
        creator: 'ses_lead',
        query: 'alpha',
        limit: 200,
      })
      expect(workspace.history.map((entry) => entry.id)).toEqual(['ses_all', 'ses_filtered'])
      expect(workspace.sidebarHistory.map((entry) => entry.id)).toEqual(['ses_filtered'])
    })
  })
})

describe('agents', () => {
  it('cycles forward and backward through the configured agents, wrapping around', async () => {
    const workspace = setup([session('ses_1', { agent: 'build' })])
    api.setAgent.mockImplementation((id: string, agent: string) => Promise.resolve(session(id, { agent })))
    await workspace.cycleAgent('ses_1', -1)
    expect(api.setAgent).toHaveBeenLastCalledWith('ses_1', 'review')
    await workspace.cycleAgent('ses_1', 1)
    expect(api.setAgent).toHaveBeenLastCalledWith('ses_1', 'build')
  })
})

describe('split view', () => {
  async function inSplit(extraSessions: ReturnType<typeof session>[] = []) {
    const workspace = setup([
      session('ses_1'),
      session('ses_2', { attached: 0 }),
      session('ses_3', { attached: 2 }),
      ...extraSessions,
    ])
    await workspace.openSession('ses_1')
    api.attach.mockClear()
    api.detach.mockClear()
    await workspace.enterSplit()
    // The backend's answer to the open session leaving the foreground.
    workspace.onFrame({
      kind: 'sessions',
      sessions: [
        session('ses_1', { attached: 0 }),
        session('ses_2', { attached: 0 }),
        session('ses_3', { attached: 2 }),
        ...extraSessions,
      ],
    })
    return workspace
  }

  it('hides the current foreground session, watches only background tiles, and starts unfocused', async () => {
    const workspace = await inSplit()
    expect(api.watchAll).toHaveBeenCalledWith(true)
    expect(api.detach).toHaveBeenCalledWith('ses_1')
    expect(api.attach).toHaveBeenCalledWith('ses_2', false)
    expect(workspace.tileIds).toEqual(['ses_2']) // ses_1 is hidden foreground, ses_3 is in another window
    expect(workspace.visibleSessionIds).toEqual(['ses_2'])
    expect(workspace.focusedId).toBeUndefined()
    expect(workspace.chat('ses_1').attachment).toBe('detached')
    expect(workspace.chat('ses_2').attachment).toBe('watching')
  })

  it('focuses a tile and allows deselecting it', async () => {
    const workspace = await inSplit()
    workspace.focusTile('ses_2')
    expect(workspace.focusedId).toBe('ses_2')
    workspace.focusTile(undefined)
    expect(workspace.focusedId).toBeUndefined()
  })

  it('zooms a tile and returns to the grid', async () => {
    const workspace = await inSplit()
    workspace.zoomTile('ses_2')
    expect(workspace.visibleSessionIds).toEqual(['ses_2'])
    expect(workspace.focusedId).toBe('ses_2')
    expect(workspace.currentId).toBe('ses_1')
    workspace.unzoom()
    expect(workspace.visibleSessionIds).toEqual(['ses_2'])
  })

  it('leaving brings the zoomed tile (or original open session) back to the foreground', async () => {
    const workspace = await inSplit()
    workspace.zoomTile('ses_2')
    api.attach.mockClear()
    api.detach.mockClear()
    await workspace.leaveSplit()
    expect(api.watchAll).toHaveBeenLastCalledWith(false)
    expect(api.attach).toHaveBeenCalledWith('ses_2', true)
    expect(workspace.mode).toBe('single')
    expect(workspace.currentId).toBe('ses_2')
    expect(workspace.zoomedId).toBeUndefined()
    expect(workspace.tileFocusId).toBeUndefined()
  })

  it('leaving without zoom re-attaches the hidden foreground session and detaches tiles', async () => {
    const workspace = await inSplit()
    api.attach.mockClear()
    api.detach.mockClear()
    await workspace.leaveSplit()
    expect(api.attach).toHaveBeenCalledWith('ses_1', true)
    expect(api.detach).toHaveBeenCalledWith('ses_2')
    expect(workspace.currentId).toBe('ses_1')
  })

  it('handles an empty split view when only the foreground session is live', async () => {
    const workspace = setup([session('ses_1')])
    await workspace.openSession('ses_1')
    await workspace.enterSplit()
    expect(workspace.tileIds).toEqual([])
    expect(workspace.focusedId).toBeUndefined()
    expect(workspace.chat('ses_1').attachment).toBe('detached')
    await workspace.leaveSplit()
    expect(workspace.chat('ses_1').attachment).toBe('foreground')
  })

  it('moves the focus to the neighbouring tile', async () => {
    const workspace = await inSplit([session('ses_4', { attached: 0 })])
    workspace.setTileRects([
      { x: 0, y: 0, width: 100, height: 100 },
      { x: 110, y: 0, width: 100, height: 100 },
    ])
    workspace.moveTileFocus(1, 0)
    expect(workspace.focusedId).toBe('ses_2')
    workspace.moveTileFocus(1, 0)
    expect(workspace.focusedId).toBe('ses_4')
    workspace.moveTileFocus(1, 0)
    expect(workspace.focusedId).toBe('ses_4')
  })

  it('leaves split view and opens the requested live or history session even when another tile is zoomed', async () => {
    const workspace = await inSplit([session('ses_4', { attached: 0 })])
    workspace.zoomTile('ses_2')
    api.attach.mockClear()
    api.detach.mockClear()
    const resumedHistory = session('ses_hist', { live: true, attached: 1 })
    api.attach.mockImplementationOnce(() =>
      Promise.resolve(snapshot([userMessage('u1', 'old')], { info: resumedHistory })),
    )

    await workspace.openSession('ses_hist')
    expect(workspace.mode).toBe('single')
    expect(workspace.currentId).toBe('ses_hist')
    expect(workspace.zoomedId).toBeUndefined()
    expect(workspace.live.some((entry) => entry.id === 'ses_hist')).toBe(true)
    expect(api.watchAll).toHaveBeenLastCalledWith(false)
    expect(api.attach).toHaveBeenCalledWith('ses_hist', true)
    expect(api.detach).toHaveBeenCalledWith('ses_2')
    expect(api.detach).toHaveBeenCalledWith('ses_4')
  })

  it('clears zoomedId when the zoomed tile session is closed or gone', async () => {
    const workspace = await inSplit([session('ses_4', { attached: 0 })])
    workspace.zoomTile('ses_2')
    expect(workspace.zoomedId).toBe('ses_2')
    await workspace.closeSession('ses_2')
    expect(workspace.zoomedId).toBeUndefined()
  })
})

describe('stash groups and splitFilter', () => {
  it('updates groups on groups_changed, supports stash RPCs, and filters split tileIds by parent', async () => {
    const group1 = { id: 'g1', name: 'Batch', locked: false, created_at: 1, updated_at: 1, members: [] }
    Object.assign(api, {
      listGroups: vi.fn().mockResolvedValue([group1]),
      stashAll: vi.fn().mockResolvedValue({ group: group1, groups: [group1] }),
      restoreGroup: vi.fn().mockResolvedValue({ restored_ids: ['ses_1'], groups: [] }),
      renameGroup: vi.fn().mockResolvedValue([{ ...group1, name: 'Renamed' }]),
      setGroupLocked: vi.fn().mockResolvedValue([{ ...group1, locked: true }]),
      deleteGroup: vi.fn().mockResolvedValue([]),
    })
    const workspace = setup([
      session('ses_1', { attached: 0, parent: null }),
      session('ses_2', { attached: 0, parent: 'ses_1' }),
      session('ses_3', { attached: 0, parent: 'ses_other' }),
    ])
    expect(workspace.tileIds).toEqual(['ses_1', 'ses_2', 'ses_3'])
    workspace.setSplitFilter('user')
    expect(workspace.tileIds).toEqual(['ses_1'])
    workspace.setSplitFilter('ses_1')
    expect(workspace.tileIds).toEqual(['ses_2'])
    workspace.setSplitFilter('all')
    expect(workspace.tileIds).toEqual(['ses_1', 'ses_2', 'ses_3'])

    workspace.onFrame({ kind: 'groups_changed', groups: [group1] })
    expect(workspace.groups).toEqual([group1])
    await workspace.renameGroup('g1', 'Renamed')
    expect(workspace.groups[0]?.name).toBe('Renamed')
    await workspace.setGroupLocked('g1', true)
    expect(workspace.groups[0]?.locked).toBe(true)
    await workspace.deleteGroup('g1')
    expect(workspace.groups).toEqual([])
    await workspace.stashAll('Batch')
    expect(workspace.groups).toEqual([group1])
    await workspace.restoreGroup('g1')
    expect(workspace.groups).toEqual([])
  })
})
