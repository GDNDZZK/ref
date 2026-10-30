// Notifications for what happens in sessions the user is not looking at.
//
//   settings, focus, visible sessions --> NotificationCenter --> SetNotify / SetFocus
//   Notify frame --> NotificationCenter --> system notification + tab alert
//
// The center holds the rules (notify/center.ts); this store connects it to the page: window
// focus, the connection, the settings and the open session.

import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { pageAttention } from '../composables/useAttention.ts'
import { translate } from '../i18n/index.ts'
import { NotificationCenter } from '../notify/center.ts'
import { browserNotifications } from '../notify/notifier.ts'
import type { NotificationPermissionState } from '../notify/notifier.ts'
import { useConnectionStore } from './connection.ts'
import { useNoticesStore } from './notices.ts'
import { useSettingsStore } from './settings.ts'
import { useWorkspaceStore } from './workspace.ts'

const NOTIFICATION_ICON = '/favicon.png'

export const useNotificationsStore = defineStore('notifications', () => {
  const connection = useConnectionStore()
  const notices = useNoticesStore()
  const settings = useSettingsStore()
  const workspace = useWorkspaceStore()

  const notifier = browserNotifications({
    Notification: typeof Notification === 'undefined' ? undefined : Notification,
    isSecureContext: window.isSecureContext,
    iconUrl: NOTIFICATION_ICON,
  })
  const availability = notifier.availability
  const permission = ref<NotificationPermissionState>(notifier.permission())

  const center = new NotificationCenter({
    notifier,
    attention: pageAttention(),
    setNotify: (kinds) => connection.getApi().setNotify(kinds),
    setFocus: (focused, sessions) => connection.getApi().setFocus(focused, sessions),
    translate: (key, params) => translate(key, params),
    isInForeground: () => document.visibilityState === 'visible' && document.hasFocus(),
    openSession: (id) => workspace.openSession(id).catch((error) => notices.pushError(error)),
    focusWindow: () => window.focus(),
    reportError: (error) => notices.pushError(error),
  })

  // Sessions on screen now. TODO(split view): all tiles, once the workspace has them.
  const visibleSessionIds = computed(() => (workspace.currentId ? [workspace.currentId] : []))

  // The browser's permission can change in its site settings while the page is open.
  function foregroundChanged(): void {
    permission.value = notifier.permission()
    center.foregroundChanged()
  }

  // Must be called from a click or key press: browsers refuse it otherwise.
  async function requestPermission(): Promise<void> {
    permission.value = await notifier.requestPermission()
  }

  function start(): void {
    watch(
      () => settings.settings.notifications,
      (value) => center.updateSettings(value),
      {
        immediate: true,
        deep: true,
      },
    )
    watch(visibleSessionIds, (ids) => center.updateVisibleSessions(ids), { immediate: true })
    watch(
      () => connection.state,
      (state) => (state === 'open' ? center.connected() : center.disconnected()),
      { immediate: true },
    )
    connection.onFrame((frame) => {
      if (frame.kind === 'notify') center.handleNotify(frame)
    })
    window.addEventListener('focus', foregroundChanged)
    window.addEventListener('blur', foregroundChanged)
    document.addEventListener('visibilitychange', foregroundChanged)
  }

  return { availability, permission, requestPermission, start }
})
