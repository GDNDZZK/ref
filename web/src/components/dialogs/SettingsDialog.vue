<script setup lang="ts">
// Theme, language and notifications.
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { setTheme } from '../../composables/useTheme.ts'
import type { LanguageChoice, ThemeChoice } from '../../settings/model.ts'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useSettingsStore } from '../../stores/settings.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import DialogButton from '../DialogButton.vue'
import ModalDialog from '../ModalDialog.vue'
import NotificationSettings from '../NotificationSettings.vue'
import Segmented from '../Segmented.vue'
const { t } = useI18n()
const dialogs = useDialogsStore()
const connection = useConnectionStore()
const store = useSettingsStore()

const notices = useNoticesStore()
const workspace = useWorkspaceStore()
const theme = computed<ThemeChoice>({
  get: () => store.settings.theme,
  set: setTheme,
})
const language = computed<LanguageChoice>({
  get: () => store.settings.language,
  set: (value) => store.update((draft) => (draft.language = value)),
})

const themes = computed(() => [
  { value: 'system' as const, label: t('dialog.settings.system') },
  { value: 'light' as const, label: t('dialog.settings.light') },
  { value: 'dark' as const, label: t('dialog.settings.dark') },
])
// The language names are not translated.
const languages = computed(() => [
  { value: 'system' as const, label: t('dialog.settings.system') },
  { value: 'en' as const, label: t('dialog.settings.english') },
  { value: 'zh-CN' as const, label: t('dialog.settings.chinese') },
])

const about = computed(() =>
  t('dialog.settings.about', {
    version: connection.hello?.server_version ?? '',
    protocol: connection.protocol,
  }),
)

function reloadConfig(): void {
  connection
    .reloadConfig(workspace.effectiveCwd)
    .then(() => notices.push('info', t('command.reload.done')))
    .catch(notices.pushError)
}
</script>

<template>
  <ModalDialog :title="t('dialog.settings.title')" width="settings" @close="dialogs.close()">
    <div class="row">
      <span>{{ t('dialog.settings.theme') }}</span>
      <Segmented v-model="theme" :options="themes" :label="t('dialog.settings.theme')" />
    </div>
    <div class="row">
      <span>{{ t('dialog.settings.language') }}</span>
      <Segmented v-model="language" :options="languages" :label="t('dialog.settings.language')" />
    </div>
    <NotificationSettings class="notifications" />
    <div class="row config-row">
      <span>{{ t('dialog.settings.config') }}</span>
      <div class="config-actions">
        <DialogButton small :label="t('dialog.settings.reloadConfig')" @click="reloadConfig" />
        <DialogButton
          small
          :label="t('dialog.settings.editConfig')"
          @click="dialogs.open({ kind: 'config' })"
        />
      </div>
    </div>
    <div class="row config-row backend-row">
      <div class="backend-info">
        <span>{{ t('dialog.settings.backend') }}</span>
        <span class="backend-detail">
          {{ connection.activeBackendUrl || t('dialog.backend.localAuto') }}
          <template v-if="connection.maskedBackendToken"> · {{ connection.maskedBackendToken }}</template>
        </span>
      </div>
      <DialogButton
        small
        :label="t('dialog.settings.switchBackend')"
        @click="dialogs.open({ kind: 'backend' })"
      />
    </div>
    <p class="about">{{ about }}</p>
  </ModalDialog>
</template>
<style scoped>
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  min-height: 48px;
}

.row + .row {
  border-top: 1px solid var(--border);
}

.notifications {
  border-top: 1px solid var(--border);
}
.config-row {
  border-top: 1px solid var(--border);
  flex-wrap: wrap;
}

.config-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.backend-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 2px;
}

.backend-detail {
  overflow: hidden;
  color: var(--text-muted);
  font: var(--small-font-size) var(--font-mono);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.about {
  margin: 16px 0 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  user-select: text;
}
</style>
