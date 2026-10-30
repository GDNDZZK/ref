import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { info, snapshot, userMessage } from '../model/testMessages.ts'
import type { SessionInfo } from '../proto/generated'
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
}
const connection = { getApi: () => api, config: undefined as unknown, loadConfig: vi.fn() }

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
  setActivePinia(createPinia())
  for (const fn of Object.values(api)) fn.mockReset().mockResolvedValue(undefined)
  api.listHistory.mockResolvedValue([])
  connection.config = { agents: [{ name: 'build' }, { name: 'plan' }, { name: 'review' }] }
})

describe('sortLive', () => {
  it('orders by creation time, then id', () => {
    const sorted = sortLive([session('ses_3'), session('ses_1'), session('b', { created_at: 3 })])
    expect(sorted.map((entry) => entry.id)).toEqual(['ses_1', 'b', 'ses_3'])
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

  it('makes a new session and opens it', async () => {
    const workspace = setup([])
    workspace.cwd = '/work'
    api.createSession.mockResolvedValue(session('ses_5'))
    expect(await workspace.newSession()).toBe('ses_5')
    expect(api.createSession).toHaveBeenCalledWith('/work', {})
    expect(workspace.currentId).toBe('ses_5')
    expect(workspace.live.map((entry) => entry.id)).toEqual(['ses_5'])
  })
})

describe('reconnecting', () => {
  const startup = { url: 'ws://x/ws', cwd: '/work', sessionId: undefined, split: false }

  it('moves on from a session that was closed while the connection was down', async () => {
    const workspace = setup([session('ses_1'), session('ses_2')])
    await workspace.openSession('ses_1')
    api.listSessions.mockResolvedValue([session('ses_2')])

    await workspace.onConnected(true, startup)

    expect(workspace.live.map((entry) => entry.id)).toEqual(['ses_2'])
    expect(workspace.currentId).toBe('ses_2')
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

  it('starts a new session in the same directory when the last one is closed', async () => {
    const workspace = setup([session('ses_1', { cwd: '/proj' })])
    await workspace.openSession('ses_1')
    api.createSession.mockResolvedValue(session('ses_2', { cwd: '/proj' }))
    await workspace.closeSession('ses_1')
    expect(api.createSession).toHaveBeenCalledWith('/proj', { cwd: '/proj' })
    expect(workspace.currentId).toBe('ses_2')
  })

  it('does not act twice when the closed event comes before the response', async () => {
    const workspace = setup([session('ses_1', { cwd: '/proj' })])
    await workspace.openSession('ses_1')
    api.createSession.mockResolvedValue(session('ses_2'))
    api.close.mockImplementation(() => {
      workspace.onFrame({ kind: 'event', session_id: 'ses_1', event: { type: 'closed' } })
      return Promise.resolve()
    })
    await workspace.closeSession('ses_1')
    await vi.waitFor(() => expect(workspace.currentId).toBe('ses_2'))
    expect(api.createSession).toHaveBeenCalledTimes(1)
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
  async function inSplit() {
    const workspace = setup([
      session('ses_1'),
      session('ses_2', { attached: 0 }),
      session('ses_3', { attached: 2 }),
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
      ],
    })
    return workspace
  }

  it('subscribes first, backgrounds the open session and watches the other tiles', async () => {
    const workspace = await inSplit()
    expect(api.watchAll).toHaveBeenCalledWith(true)
    expect(api.detach).toHaveBeenCalledWith('ses_1')
    expect(api.attach).toHaveBeenCalledWith('ses_2', false)
    expect(workspace.tileIds).toEqual(['ses_1', 'ses_2']) // ses_3 is open in another window
    expect(workspace.visibleSessionIds).toEqual(['ses_1', 'ses_2'])
    expect(workspace.focusedId).toBe('ses_1')
    expect(workspace.chat('ses_1').attachment).toBe('watching')
  })

  it('zooms a tile and returns to the grid', async () => {
    const workspace = await inSplit()
    workspace.zoomTile('ses_2')
    expect(workspace.visibleSessionIds).toEqual(['ses_2'])
    expect(workspace.focusedId).toBe('ses_2')
    workspace.unzoom()
    expect(workspace.visibleSessionIds).toEqual(['ses_1', 'ses_2'])
  })

  it('leaving brings the open session back to the foreground and lets go of the others', async () => {
    const workspace = await inSplit()
    workspace.zoomTile('ses_2')
    api.attach.mockClear()
    api.detach.mockClear()
    await workspace.leaveSplit()
    expect(api.watchAll).toHaveBeenLastCalledWith(false)
    expect(api.attach).toHaveBeenCalledWith('ses_2', true)
    expect(api.detach).toHaveBeenCalledWith('ses_1')
    expect(workspace.mode).toBe('single')
    expect(workspace.currentId).toBe('ses_2')
    expect(workspace.zoomedId).toBeUndefined()
  })

  it('moves the focus to the neighbouring tile', async () => {
    const workspace = await inSplit()
    workspace.setTileRects([
      { x: 0, y: 0, width: 100, height: 100 },
      { x: 110, y: 0, width: 100, height: 100 },
    ])
    workspace.moveTileFocus(1, 0)
    expect(workspace.focusedId).toBe('ses_2')
    workspace.moveTileFocus(1, 0)
    expect(workspace.focusedId).toBe('ses_2')
  })
})
