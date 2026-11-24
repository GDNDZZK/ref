<script setup lang="ts">
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { maskToken, validateBackendUrl } from '../../net/backend.ts'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore } from '../../stores/dialogs.ts'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'
import ModalDialog from '../ModalDialog.vue'
import TextField from '../TextField.vue'

const { t } = useI18n()
const connection = useConnectionStore()
const dialogs = useDialogsStore()

const urlField = useTemplateRef<InstanceType<typeof TextField>>('urlField')
const existingOverride = computed(() => connection.temporaryBackend ?? connection.savedBackend)
const url = ref(existingOverride.value?.url ?? (connection.backendSource === 'direct' ? connection.activeBackendUrl : ''))
const tokenInput = ref('')
const tokenCleared = ref(false)
const showToken = ref(false)
const urlError = ref('')

const savedToken = computed(() => (tokenCleared.value ? undefined : existingOverride.value?.token))
const maskedExistingToken = computed(() => maskToken(savedToken.value))

const sourceLabel = computed(() => {
  switch (connection.backendSource) {
    case 'temporary':
      return t('dialog.backend.sourceTemporary')
    case 'saved':
      return t('dialog.backend.sourceSaved')
    case 'direct':
      return t('dialog.backend.sourceDirect')
    default:
      return t('dialog.backend.sourceAuto')
  }
})

const effectiveToken = (): string | undefined => {
  const typed = tokenInput.value.trim()
  if (typed !== '') return typed
  return savedToken.value
}

function validate(): string | undefined {
  const result = validateBackendUrl(url.value)
  if (!result.ok) {
    urlError.value = t(result.errorKey)
    urlField.value?.focus()
    return undefined
  }
  urlError.value = ''
  return result.url
}

function switchTemporarily(): void {
  const validUrl = validate()
  if (!validUrl) return
  connection.switchBackend({ mode: 'temporary', url: validUrl, token: effectiveToken() })
  dialogs.close()
}

function saveAsDefault(): void {
  const validUrl = validate()
  if (!validUrl) return
  connection.switchBackend({ mode: 'saveDefault', url: validUrl, token: effectiveToken() })
  dialogs.close()
}

function restoreLocalDefault(): void {
  connection.switchBackend({ mode: 'restoreDefault' })
  dialogs.close()
}

function clearToken(): void {
  tokenInput.value = ''
  tokenCleared.value = true
}

onMounted(() => urlField.value?.focus())
</script>

<template>
  <ModalDialog :title="t('dialog.backend.title')" @close="dialogs.close()">
    <div class="current">
      <div class="current-row">
        <span class="current-label">{{ t('dialog.backend.current') }}</span>
        <span class="source-badge">{{ sourceLabel }}</span>
      </div>
      <p class="current-url">{{ connection.activeBackendUrl || t('dialog.backend.localAuto') }}</p>
      <p v-if="connection.maskedBackendToken" class="current-token">
        {{ t('dialog.backend.tokenMasked', { token: connection.maskedBackendToken }) }}
      </p>
    </div>

    <div class="field-group">
      <label class="label">{{ t('dialog.backend.urlLabel') }}</label>
      <TextField
        ref="urlField"
        v-model="url"
        :label="t('dialog.backend.urlLabel')"
        :placeholder="t('dialog.backend.urlPlaceholder')"
        :error="urlError"
        @update:model-value="urlError = ''"
        @enter="switchTemporarily"
      />
    </div>

    <div class="field-group">
      <div class="label-row">
        <label class="label" for="backend-token-input">{{ t('dialog.backend.tokenLabel') }}</label>
        <button
          v-if="savedToken && !tokenInput"
          type="button"
          class="clear-token"
          @click="clearToken"
        >
          {{ t('dialog.backend.clearToken') }}
        </button>
      </div>
      <div class="token-wrap">
        <input
          id="backend-token-input"
          v-model="tokenInput"
          :type="showToken ? 'text' : 'password'"
          class="token-input"
          spellcheck="false"
          autocomplete="off"
          :placeholder="maskedExistingToken ? t('dialog.backend.keepToken', { token: maskedExistingToken }) : t('dialog.backend.tokenPlaceholder')"
          @keydown.enter.prevent="switchTemporarily"
        />
        <button
          type="button"
          class="toggle-eye"
          :aria-label="showToken ? t('config.secret.hide') : t('config.secret.show')"
          :title="showToken ? t('config.secret.hide') : t('config.secret.show')"
          @click="showToken = !showToken"
        >
          <Icon :icon="showToken ? 'eye-off-16' : 'eye-16'" />
        </button>
      </div>
    </div>

    <template #footer>
      <div class="footer-actions">
        <DialogButton
          class="restore-btn"
          :label="t('dialog.backend.restoreDefault')"
          @click="restoreLocalDefault"
        />
        <div class="right-actions">
          <DialogButton
            :label="t('dialog.backend.switchTemporary')"
            @click="switchTemporarily"
          />
          <DialogButton
            kind="primary"
            :label="t('dialog.backend.saveDefault')"
            @click="saveAsDefault"
          />
        </div>
      </div>
    </template>
  </ModalDialog>
</template>

<style scoped>
.current {
  margin-bottom: 14px;
  padding: 10px 12px;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
}

.current-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.current-label {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.source-badge {
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--surface-hover);
  color: var(--text);
  font-size: var(--micro-font-size);
}

.current-url {
  margin: 4px 0 0;
  overflow: hidden;
  font: var(--small-font-size) var(--font-mono);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.current-token {
  margin: 4px 0 0;
  color: var(--text-muted);
  font: var(--small-font-size) var(--font-mono);
}

.field-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.field-group + .field-group {
  margin-top: 12px;
}

.label-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.label {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.clear-token {
  color: var(--accent-text);
  font-size: var(--small-font-size);
}

.clear-token:hover {
  text-decoration: underline;
}

.token-wrap {
  position: relative;
  display: flex;
  align-items: center;
}

.token-input {
  width: 100%;
  height: var(--field-height);
  padding: 0 38px 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  transition: box-shadow var(--motion-standard) var(--motion-ease);
}

.token-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.toggle-eye {
  position: absolute;
  right: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
}

.toggle-eye:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.footer-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
}

.right-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-left: auto;
}
</style>
