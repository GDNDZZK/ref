import { describe, expect, it, vi } from 'vitest'
import type { NotifyKind } from '../proto/generated'
import type { NotificationSettings } from '../settings/model.ts'
import { NotificationCenter } from './center.ts'
import type { CenterDeps, NotifyFrame } from './center.ts'
import type { NotificationPermissionState, NotificationRequest } from './notifier.ts'

const ALL_ON: NotificationSettings = { enabled: true, permission: true, turnDone: true, turnError: true }

function setup(permission: NotificationPermissionState = 'granted') {
  const state = { inForeground: true, permission, showError: undefined as Error | undefined }
  const shown: NotificationRequest[] = []
  const calls = {
    setNotify: vi.fn<(kinds: NotifyKind[]) => Promise<void>>(async () => {}),
    setFocus: vi.fn<(focused: boolean, sessions: string[]) => Promise<void>>(async () => {}),
    setAlerting: vi.fn<(alerting: boolean) => void>(),
    openSession: vi.fn<(id: string) => void>(),
    focusWindow: vi.fn<() => void>(),
    reportError: vi.fn<(error: unknown) => void>(),
  }
  const deps: CenterDeps = {
    notifier: {
      availability: 'available',
      permission: () => state.permission,
      requestPermission: async () => state.permission,
      show: (request) => {
        if (state.showError) throw state.showError
        shown.push(request)
      },
    },
    attention: { setBaseTitle: () => {}, setAlerting: calls.setAlerting },
    setNotify: calls.setNotify,
    setFocus: calls.setFocus,
    translate: (key, params) => (params ? `${key} ${JSON.stringify(params)}` : key),
    isInForeground: () => state.inForeground,
    openSession: calls.openSession,
    focusWindow: calls.focusWindow,
    reportError: calls.reportError,
  }
  return { center: new NotificationCenter(deps), state, shown, calls }
}

function frame(overrides: Partial<NotifyFrame> = {}): NotifyFrame {
  return { session_id: 's1', notify_kind: 'turn_done', session_title: 'Fix bug', detail: null, ...overrides }
}

describe('SetNotify', () => {
  it('is not sent for what the handshake carried', () => {
    const { center, calls } = setup()
    center.updateSettings(ALL_ON)
    center.connected()
    expect(calls.setNotify).not.toHaveBeenCalled()
  })

  it('is sent when the settings change while connected', () => {
    const { center, calls } = setup()
    center.updateSettings(ALL_ON)
    center.connected()
    center.updateSettings({ ...ALL_ON, turnError: false })
    center.updateSettings({ ...ALL_ON, turnError: false })
    expect(calls.setNotify.mock.calls).toEqual([[['permission', 'question', 'turn_done']]])
  })

  it('waits for the connection', () => {
    const { center, calls } = setup()
    center.updateSettings(ALL_ON)
    center.updateSettings({ ...ALL_ON, enabled: false })
    expect(calls.setNotify).not.toHaveBeenCalled()
  })

  it('reports a failed request', async () => {
    const { center, calls } = setup()
    const failure = new Error('lost')
    calls.setNotify.mockRejectedValueOnce(failure)
    center.connected()
    center.updateSettings(ALL_ON)
    await Promise.resolve()
    expect(calls.reportError).toHaveBeenCalledWith(failure)
  })
})

describe('SetFocus', () => {
  it('is sent on connecting and when the sessions on screen change', () => {
    const { center, calls } = setup()
    center.updateVisibleSessions(['s1'])
    expect(calls.setFocus).not.toHaveBeenCalled()
    center.connected()
    center.updateVisibleSessions(['s1', 's2'])
    expect(calls.setFocus.mock.calls).toEqual([
      [true, ['s1']],
      [true, ['s1', 's2']],
    ])
  })

  it('is sent when the page goes to the background, once', () => {
    const { center, state, calls } = setup()
    center.updateVisibleSessions(['s1'])
    center.connected()
    state.inForeground = false
    center.foregroundChanged()
    center.foregroundChanged()
    expect(calls.setFocus.mock.calls).toEqual([
      [true, ['s1']],
      [false, ['s1']],
    ])
  })

  it('is sent again after a reconnect', () => {
    const { center, calls } = setup()
    center.connected()
    center.disconnected()
    center.updateVisibleSessions(['s1'])
    center.connected()
    expect(calls.setFocus.mock.calls).toEqual([
      [true, []],
      [true, ['s1']],
    ])
  })
})

describe('Notify frames', () => {
  it('says what a question asks', () => {
    const { center, shown } = setup()
    center.handleNotify(frame({ notify_kind: 'question', detail: 'Which database?' }))
    center.handleNotify(frame({ notify_kind: 'question', detail: null }))
    expect(shown.map((item) => [item.body, item.tag])).toEqual([
      ['notify.questionDetail {"question":"Which database?"}', 's1:question'],
      ['notify.question', 's1:question'],
    ])
  })

  it('show a system notification that opens the session when clicked', () => {
    const { center, shown, calls } = setup()
    center.handleNotify(frame({ notify_kind: 'permission', detail: 'bash' }))
    expect(shown).toHaveLength(1)
    expect(shown[0]).toMatchObject({
      title: 'Fix bug',
      body: 'notify.permissionTool {"tool":"bash"}',
      tag: 's1:permission',
    })
    shown[0]!.onClick()
    expect(calls.focusWindow).toHaveBeenCalled()
    expect(calls.openSession).toHaveBeenCalledWith('s1')
  })

  it('build the text from the kind', () => {
    const { center, shown } = setup()
    center.handleNotify(frame({ notify_kind: 'turn_done' }))
    center.handleNotify(frame({ notify_kind: 'turn_error', detail: 'rate limited' }))
    center.handleNotify(frame({ notify_kind: 'turn_error', detail: null, session_title: '' }))
    expect(shown.map((n) => [n.title, n.body])).toEqual([
      ['Fix bug', 'notify.turnDone'],
      ['Fix bug', 'notify.turnErrorDetail {"error":"rate limited"}'],
      ['ref', 'notify.turnError'],
    ])
  })

  it('cut a very long error', () => {
    const { center, shown } = setup()
    center.handleNotify(frame({ notify_kind: 'turn_error', detail: 'x'.repeat(1000) }))
    expect(shown[0]!.body.length).toBeLessThan(400)
    expect(shown[0]!.body).toContain('…')
  })

  it('only alert the tab while the page is not in front', () => {
    const { center, state, calls } = setup()
    center.handleNotify(frame())
    expect(calls.setAlerting).not.toHaveBeenCalled()

    state.inForeground = false
    center.handleNotify(frame())
    expect(calls.setAlerting).toHaveBeenCalledWith(true)

    state.inForeground = true
    center.foregroundChanged()
    expect(calls.setAlerting).toHaveBeenLastCalledWith(false)
  })

  it('still alert the tab without permission, but show no system notification', () => {
    const { center, state, shown, calls } = setup('default')
    state.inForeground = false
    center.handleNotify(frame())
    expect(shown).toEqual([])
    expect(calls.setAlerting).toHaveBeenCalledWith(true)
  })

  it('report a notification the browser refused to show', () => {
    const { center, state, calls } = setup()
    const failure = new TypeError('Illegal constructor')
    state.showError = failure
    center.handleNotify(frame())
    expect(calls.reportError).toHaveBeenCalledWith(failure)
  })
})
