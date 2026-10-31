// The settings the user can change and how they are stored (the browser counterpart of gui.json).

import type { NotifyKind } from '../proto/generated'

export type ThemeChoice = 'system' | 'light' | 'dark'
export type LanguageChoice = 'system' | 'en' | 'zh-CN'

export interface NotificationSettings {
  enabled: boolean // master switch
  permission: boolean
  turnDone: boolean
  turnError: boolean
}

export interface Settings {
  theme: ThemeChoice
  language: LanguageChoice
  notifications: NotificationSettings
  sidebarOpen: boolean
}

export const STORAGE_KEY = 'ref.settings'

export const THEME_CHOICES: readonly ThemeChoice[] = ['system', 'light', 'dark']
export const LANGUAGE_CHOICES: readonly LanguageChoice[] = ['system', 'en', 'zh-CN']

export function defaultSettings(): Settings {
  return {
    theme: 'system',
    language: 'system',
    notifications: { enabled: true, permission: true, turnDone: true, turnError: true },
    sidebarOpen: true,
  }
}

function pick<T extends string>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback
}

function flag(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : {}
}

// Every field falls back on its own, so one bad value (or a field a newer version added) does not
// reset the rest.
export function parseSettings(raw: string | null): Settings {
  const defaults = defaultSettings()
  if (raw === null) return defaults

  let stored: Record<string, unknown>
  try {
    stored = record(JSON.parse(raw))
  } catch {
    return defaults // not JSON: the key is not ours to repair
  }

  const notifications = record(stored['notifications'])
  return {
    theme: pick(stored['theme'], THEME_CHOICES, defaults.theme),
    language: pick(stored['language'], LANGUAGE_CHOICES, defaults.language),
    notifications: {
      enabled: flag(notifications['enabled'], defaults.notifications.enabled),
      permission: flag(notifications['permission'], defaults.notifications.permission),
      turnDone: flag(notifications['turnDone'], defaults.notifications.turnDone),
      turnError: flag(notifications['turnError'], defaults.notifications.turnError),
    },
    sidebarOpen: flag(stored['sidebarOpen'], defaults.sidebarOpen),
  }
}

// The notification kinds this browser asks the backend for (Hello.notify, SetNotify).
export function notifyKinds(notifications: NotificationSettings): NotifyKind[] {
  if (!notifications.enabled) return []
  const wanted: [boolean, NotifyKind][] = [
    [notifications.permission, 'permission'],
    [notifications.permission, 'question'], // a question needs the user like a permission request
    [notifications.turnDone, 'turn_done'],
    [notifications.turnError, 'turn_error'],
  ]
  return wanted.filter(([on]) => on).map(([, kind]) => kind)
}
