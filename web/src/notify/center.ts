// Decides what the page tells the backend and the user about notifications.
//
//   settings ----> Hello.notify when connecting, SetNotify when they change later
//   focus + visible sessions ----> SetFocus (so the backend stays quiet about what is on screen)
//   Notify frame ----> system notification (if permitted) + tab alert (if the page is not in front)
//
// Everything outside the page (API, Notification, document) comes in through `CenterDeps`.

import type { NotifyKind } from '../proto/generated'
import { notifyKinds } from '../settings/model.ts'
import type { NotificationSettings } from '../settings/model.ts'
import type { Attention } from './attention.ts'
import type { BrowserNotifications } from './notifier.ts'

export interface NotifyFrame {
  session_id: string
  notify_kind: NotifyKind
  session_title: string
  detail: string | null
}

export interface CenterDeps {
  notifier: BrowserNotifications
  attention: Attention
  setNotify(kinds: NotifyKind[]): Promise<void>
  setFocus(focused: boolean, sessions: string[]): Promise<void>
  // Localized text; keys are under `notify.` in the locale files.
  translate(key: string, params?: Record<string, string>): string
  isInForeground(): boolean // the tab is visible and has the focus
  openSession(sessionId: string): void
  focusWindow(): void
  reportError(error: unknown): void
}

const DEFAULT_TITLE = 'ref'
const MAX_DETAIL_CHARS = 300

export class NotificationCenter {
  private readonly deps: CenterDeps
  private settings: NotificationSettings | undefined
  private visibleSessions: string[] = []
  private isOpen = false // the connection is up; requests would fail otherwise
  private sentKinds: string | undefined // what the backend has, as a comparable string
  private sentFocus: string | undefined

  constructor(deps: CenterDeps) {
    this.deps = deps
  }

  updateSettings(settings: NotificationSettings): void {
    this.settings = settings
    this.syncKinds()
  }

  updateVisibleSessions(sessionIds: readonly string[]): void {
    this.visibleSessions = [...sessionIds]
    this.syncFocus()
  }

  // The page came to the front or went away.
  foregroundChanged(): void {
    if (this.deps.isInForeground()) this.deps.attention.setAlerting(false)
    this.syncFocus()
  }

  // The connection opened or came back. The handshake carried the wanted kinds (`notifyKinds`),
  // but the backend knows nothing about this tab's focus yet.
  connected(): void {
    this.isOpen = true
    this.sentKinds = this.settings && notifyKinds(this.settings).join(',')
    this.sentFocus = undefined
    this.syncKinds()
    this.syncFocus()
  }

  disconnected(): void {
    this.isOpen = false
  }

  handleNotify(frame: NotifyFrame): void {
    if (!this.deps.isInForeground()) this.deps.attention.setAlerting(true)
    if (this.deps.notifier.permission() !== 'granted') return

    const { session_id: sessionId, notify_kind: kind } = frame
    try {
      this.deps.notifier.show({
        title: frame.session_title || DEFAULT_TITLE,
        body: this.bodyOf(kind, frame.detail),
        tag: `${sessionId}:${kind}`,
        onClick: () => this.openSession(sessionId),
      })
    } catch (error) {
      this.deps.reportError(error)
    }
  }

  private openSession(sessionId: string): void {
    this.deps.focusWindow()
    this.deps.openSession(sessionId)
  }

  private bodyOf(kind: NotifyKind, detail: string | null): string {
    const text = detail === null ? undefined : shorten(detail)
    switch (kind) {
      case 'permission':
        return text === undefined
          ? this.deps.translate('notify.permission')
          : this.deps.translate('notify.permissionTool', { tool: text })
      case 'question':
        return text === undefined
          ? this.deps.translate('notify.question')
          : this.deps.translate('notify.questionDetail', { question: text })
      case 'turn_done':
        return this.deps.translate('notify.turnDone')
      case 'turn_error':
        return text === undefined
          ? this.deps.translate('notify.turnError')
          : this.deps.translate('notify.turnErrorDetail', { error: text })
    }
  }

  private syncKinds(): void {
    if (!this.settings || !this.isOpen) return
    const kinds = notifyKinds(this.settings)
    const key = kinds.join(',')
    if (key === this.sentKinds) return
    this.sentKinds = key
    this.deps.setNotify(kinds).catch((error) => this.deps.reportError(error))
  }

  private syncFocus(): void {
    if (!this.isOpen) return
    const focused = this.deps.isInForeground()
    const key = `${focused}:${this.visibleSessions.join(',')}`
    if (key === this.sentFocus) return
    this.sentFocus = key
    this.deps.setFocus(focused, this.visibleSessions).catch((error) => this.deps.reportError(error))
  }
}

function shorten(text: string): string {
  const chars = Array.from(text.trim())
  if (chars.length <= MAX_DETAIL_CHARS) return chars.join('')
  return chars.slice(0, MAX_DETAIL_CHARS).join('') + '…'
}
