<script setup lang="ts">
// Theme, language and notifications.
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { setTheme } from '../../composables/useTheme.ts'
import {
  DEFAULT_MESSAGE_WINDOW_SIZE,
  MESSAGE_WINDOW_STEP,
  type LanguageChoice,
  type ThemeChoice,
} from '../../settings/model.ts'
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
const messageWindow = computed(() => store.settings.messageWindowSize)
const windowDraft = ref(String(store.settings.messageWindowSize))
watch(
  () => store.settings.messageWindowSize,
  (next) => {
    windowDraft.value = String(next)
  },
)

function parseWindowInput(raw: string): number | undefined {
  const trimmed = String(raw).trim()
  if (trimmed === '') return undefined
  const parsed = Number(trimmed)
  if (!Number.isFinite(parsed) || parsed < 0) return undefined
  return Math.round(parsed)
}

function setMessageWindow(next: number): void {
  const normalized = Math.max(0, Math.round(next))
  windowDraft.value = String(normalized)
  if (normalized !== store.settings.messageWindowSize) {
    store.update((draft) => {
      draft.messageWindowSize = normalized
    })
  }
}

function stepMessageWindow(delta: number): void {
  const base = parseWindowInput(String(windowDraft.value)) ?? store.settings.messageWindowSize
  setMessageWindow(Math.max(0, base + delta))
}

function commitWindowInput(): void {
  const parsed = parseWindowInput(String(windowDraft.value))
  if (parsed === undefined) {
    windowDraft.value = String(store.settings.messageWindowSize)
    return
  }
  setMessageWindow(parsed)
}

function onWindowEsc(event: KeyboardEvent): void {
  const saved = String(store.settings.messageWindowSize)
  if (String(windowDraft.value) !== saved) {
    event.preventDefault()
    event.stopPropagation()
    windowDraft.value = saved
  }
}

function resetMessageWindow(): void {
  setMessageWindow(DEFAULT_MESSAGE_WINDOW_SIZE)
}

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
    <div class="row window-row">
      <div class="window-info">
        <span>{{ t('dialog.settings.messageWindow') }}</span>
        <span class="window-hint">{{ t('dialog.settings.messageWindowHint') }}</span>
      </div>
      <div class="window-controls">
        <DialogButton
          v-if="messageWindow !== DEFAULT_MESSAGE_WINDOW_SIZE"
          small
          :label="t('dialog.settings.reset')"
          @click="resetMessageWindow"
        />
        <div class="stepper">
          <button
            type="button"
            class="stepper-btn"
            :disabled="messageWindow <= 0"
            :aria-label="`-${MESSAGE_WINDOW_STEP}`"
            @click="stepMessageWindow(-MESSAGE_WINDOW_STEP)"
          >
            −
          </button>
          <input
            v-model="windowDraft"
            type="text"
            inputmode="numeric"
            class="stepper-input"
            :aria-label="t('dialog.settings.messageWindow')"
            @keydown.enter.prevent="commitWindowInput"
            @keydown.esc="onWindowEsc"
            @keydown.arrow-up.prevent="stepMessageWindow(MESSAGE_WINDOW_STEP)"
            @keydown.arrow-down.prevent="stepMessageWindow(-MESSAGE_WINDOW_STEP)"
            @blur="commitWindowInput"
          />
          <button
            type="button"
            class="stepper-btn"
            :aria-label="`+${MESSAGE_WINDOW_STEP}`"
            @click="stepMessageWindow(MESSAGE_WINDOW_STEP)"
          >
            +
          </button>
        </div>
      </div>
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
.window-row {
  flex-wrap: wrap;
}

.window-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 2px;
}

.window-hint {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.window-controls {
  display: flex;
  align-items: center;
  gap: 8px;
}

.stepper {
  display: inline-flex;
  align-items: center;
  height: 28px;
  border-radius: 14px;
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  overflow: hidden;
}

.stepper:focus-within {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.stepper-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 100%;
  color: var(--text-muted);
  font-size: var(--font-size);
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.stepper-btn:not(:disabled):hover {
  background: var(--surface-hover);
  color: var(--text);
}

.stepper-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.stepper-input {
  width: 68px;
  height: 100%;
  padding: 0 4px;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: calc(var(--font-size) - 1px);
  font-variant-numeric: tabular-nums;
  text-align: center;
  appearance: textfield;
  -moz-appearance: textfield;
}

.stepper-input::-webkit-inner-spin-button,
.stepper-input::-webkit-outer-spin-button {
  -webkit-appearance: none;
  margin: 0;
}
.about {
  margin: 16px 0 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  user-select: text;
}
</style>
