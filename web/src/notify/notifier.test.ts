import { describe, expect, it } from 'vitest'
import { browserNotifications } from './notifier.ts'
import type { NotificationConstructor, NotificationPermissionState } from './notifier.ts'

class FakeNotification {
  static permission: NotificationPermissionState = 'default'
  static granted: NotificationPermissionState = 'granted'
  static created: FakeNotification[] = []
  static requestPermission = async () => (FakeNotification.permission = FakeNotification.granted)

  onclick: (() => void) | null = null
  closed = false
  readonly title: string
  readonly options: { body?: string; tag?: string; icon?: string } | undefined

  constructor(title: string, options?: { body?: string; tag?: string; icon?: string }) {
    this.title = title
    this.options = options
    FakeNotification.created.push(this)
  }

  close(): void {
    this.closed = true
  }
}

function environment(overrides: { secure?: boolean; native?: boolean } = {}) {
  FakeNotification.permission = 'default'
  FakeNotification.granted = 'granted'
  FakeNotification.created = []
  return {
    Notification: overrides.native === false ? undefined : (FakeNotification as NotificationConstructor),
    isSecureContext: overrides.secure ?? true,
    iconUrl: '/favicon.png',
  }
}

describe('browserNotifications', () => {
  it('is available in a secure context with the API', () => {
    expect(browserNotifications(environment()).availability).toBe('available')
  })

  it('is not available over plain http', () => {
    const notifier = browserNotifications(environment({ secure: false }))
    expect(notifier.availability).toBe('insecure')
    expect(notifier.permission()).toBe('denied')
  })

  it('is not available without the API', () => {
    expect(browserNotifications(environment({ native: false })).availability).toBe('unsupported')
  })

  it('asks for permission and tells the answer', async () => {
    const notifier = browserNotifications(environment())
    expect(notifier.permission()).toBe('default')
    expect(await notifier.requestPermission()).toBe('granted')
    expect(notifier.permission()).toBe('granted')
  })

  it('does not ask where notifications are not available', async () => {
    const notifier = browserNotifications(environment({ secure: false }))
    expect(await notifier.requestPermission()).toBe('denied')
  })

  it('shows a notification; a click closes it and calls back', () => {
    const notifier = browserNotifications(environment())
    let clicked = 0
    notifier.show({ title: 'Fix bug', body: 'Done', tag: 's1:turn_done', onClick: () => clicked++ })
    const shown = FakeNotification.created[0]!
    expect(shown.title).toBe('Fix bug')
    expect(shown.options).toEqual({ body: 'Done', tag: 's1:turn_done', icon: '/favicon.png' })
    shown.onclick!()
    expect(shown.closed).toBe(true)
    expect(clicked).toBe(1)
  })

  it('shows nothing where notifications are not available', () => {
    const notifier = browserNotifications(environment({ secure: false }))
    notifier.show({ title: 't', body: 'b', tag: 'x', onClick: () => {} })
    expect(FakeNotification.created).toEqual([])
  })
})
