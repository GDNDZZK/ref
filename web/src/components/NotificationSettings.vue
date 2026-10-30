<script setup lang="ts">
// The "Notifications" block of the settings panel (NotificationSettingsSection.qml): a master
// switch and one switch per kind, plus a note when the browser cannot or will not show them.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { NotificationSettings } from '../settings/model.ts'
import { useNotificationsStore } from '../stores/notifications.ts'
import { useSettingsStore } from '../stores/settings.ts'
import Icon from './Icon.vue'
import TextButton from './TextButton.vue'
import ToggleSwitch from './ToggleSwitch.vue'

type KindKey = Exclude<keyof NotificationSettings, 'enabled'>

const KINDS: { key: KindKey; labelKey: string }[] = [
  { key: 'permission', labelKey: 'notifySettings.permission' },
  { key: 'turnDone', labelKey: 'notifySettings.turnDone' },
  { key: 'turnError', labelKey: 'notifySettings.turnError' },
]

const { t } = useI18n()
const settings = useSettingsStore()
const notifications = useNotificationsStore()

const current = computed(() => settings.settings.notifications)

// Why system notifications will not appear, or null when they will (or can still be allowed).
const unavailableNoteKey = computed(() => {
  if (notifications.availability === 'insecure') return 'notifySettings.insecure'
  if (notifications.availability === 'unsupported') return 'notifySettings.unsupported'
  if (notifications.permission === 'denied') return 'notifySettings.denied'
  return null
})

const canAsk = computed(
  () =>
    notifications.availability === 'available' &&
    notifications.permission === 'default' &&
    current.value.enabled,
)

function setEnabled(enabled: boolean): void {
  settings.update((draft) => (draft.notifications.enabled = enabled))
  // Called from the click, which the browser requires for the permission prompt.
  if (enabled && notifications.permission === 'default') void notifications.requestPermission()
}

function setKind(key: KindKey, value: boolean): void {
  settings.update((draft) => (draft.notifications[key] = value))
}
</script>

<template>
  <section class="notification-settings">
    <header class="header">
      <Icon icon="bell-20" class="bell" />
      <h3>{{ t('notifySettings.title') }}</h3>
    </header>

    <div class="row">
      <span class="label">{{ t('notifySettings.enable') }}</span>
      <ToggleSwitch
        :model-value="current.enabled"
        :label="t('notifySettings.enable')"
        @update:model-value="setEnabled"
      />
    </div>
    <p v-if="unavailableNoteKey" class="note">{{ t(unavailableNoteKey) }}</p>
    <div v-else-if="canAsk" class="row ask">
      <span class="note">{{ t('notifySettings.needsPermission') }}</span>
      <TextButton :label="t('notifySettings.allow')" @click="notifications.requestPermission()" />
    </div>

    <div v-for="kind in KINDS" :key="kind.key" class="row kind" :class="{ dimmed: !current.enabled }">
      <span class="label">{{ t(kind.labelKey) }}</span>
      <ToggleSwitch
        :model-value="current[kind.key]"
        :label="t(kind.labelKey)"
        :disabled="!current.enabled"
        @update:model-value="setKind(kind.key, $event)"
      />
    </div>
  </section>
</template>

<style scoped>
.header {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 40px;
}

.bell {
  color: var(--text-muted);
}

h3 {
  margin: 0;
  font-size: var(--font-size);
  font-weight: 600;
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 40px;
  font-size: var(--font-size);
}

.kind {
  padding-left: 24px;
  transition: opacity var(--motion-standard) var(--motion-ease);
}

.dimmed {
  opacity: 0.4;
  pointer-events: none;
}

.note {
  margin: 0;
  font-size: var(--small-font-size);
  color: var(--text-muted);
}

.ask {
  min-height: 32px;
}
</style>
