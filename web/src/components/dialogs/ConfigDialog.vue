<script setup lang="ts">
// Graphical and raw JSONC editor for global `ref.json` and `ref.json.<n>.bak` profiles.
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { parseJsoncObject } from '../../config/jsonc.ts'
import { cloneJson, formatJson, isPlainObject, type JsonObject } from '../../config/jsonPatch.ts'
import { describeError } from '../../errorMessage.ts'
import type { IconKey } from '../../icons/generated/icons.ts'
import type { ConfigFileEntry, ConfigFilesInfo } from '../../proto/generated'
import { useConnectionStore } from '../../stores/connection.ts'
import { useDialogsStore, type ConfigSectionId } from '../../stores/dialogs.ts'
import { useNoticesStore } from '../../stores/notices.ts'
import { useWorkspaceStore } from '../../stores/workspace.ts'
import Chevron from '../Chevron.vue'
import DialogButton from '../DialogButton.vue'
import Icon from '../Icon.vue'
import Segmented from '../Segmented.vue'
import ToggleSwitch from '../ToggleSwitch.vue'
import { TextAreaField } from '../config/index.ts'
import AgentsSection from '../config/sections/AgentsSection.vue'
import CommandsSection from '../config/sections/CommandsSection.vue'
import GeneralSection from '../config/sections/GeneralSection.vue'
import McpSection from '../config/sections/McpSection.vue'
import PermissionsSection from '../config/sections/PermissionsSection.vue'
import ProvidersSection from '../config/sections/ProvidersSection.vue'

type EditorMode = 'visual' | 'json'

type PendingDirtyAction =
  { kind: 'close' } | { kind: 'select'; file: string } | { kind: 'reload' } | { kind: 'openCreate' }

const NAV_ITEMS: readonly { id: ConfigSectionId; icon: IconKey }[] = [
  { id: 'general', icon: 'file-text-16' },
  { id: 'providers', icon: 'model-16' },
  { id: 'agents', icon: 'agent-16' },
  { id: 'permissions', icon: 'lock-16' },
  { id: 'mcp', icon: 'mcp-16' },
  { id: 'commands', icon: 'terminal-16' },
]

const props = withDefaults(defineProps<{ section?: ConfigSectionId }>(), {
  section: undefined,
})

const { t } = useI18n()
const connection = useConnectionStore()
const workspace = useWorkspaceStore()
const dialogs = useDialogsStore()
const notices = useNoticesStore()

const activeSection = ref<ConfigSectionId>(props.section ?? 'general')
const editorMode = ref<EditorMode>('visual')
const loading = ref(true)
const busy = ref(false)
const actionError = ref<string | null>(null)

const filesInfo = shallowRef<ConfigFilesInfo>()
const savedRawText = ref('')
const rawText = ref('')
const doc = shallowRef<JsonObject>({})
const jsonParseError = ref<string | null>(null)

const creatingProfile = ref(false)
const newProfileLabel = ref('')
const cloneCurrent = ref(true)
const confirmingDelete = ref(false)
const pendingDirtyAction = ref<PendingDirtyAction | null>(null)

watch(
  () => props.section,
  (next) => {
    if (next) activeSection.value = next
  },
)

const dirty = computed(() => !loading.value && rawText.value !== savedRawText.value)

const modeOptions = computed(() => [
  { value: 'visual' as const, label: t('config.editor.modeVisual') },
  { value: 'json' as const, label: t('config.editor.modeJson') },
])

const files = computed<readonly ConfigFileEntry[]>(() => filesInfo.value?.files ?? [])
const selectedFile = computed(() => filesInfo.value?.selected_file ?? 'ref.json')
const activeEntry = computed(() => files.value.find((entry) => entry.active))
const backupCount = computed(() => files.value.filter((entry) => !entry.active).length)
const canDelete = computed(() => (activeEntry.value?.exists ?? false) || backupCount.value > 0)

const deletePromptText = computed(() => {
  if (selectedFile.value === 'ref.json') {
    return backupCount.value > 0
      ? t('config.editor.confirmDeleteActiveWithBackup')
      : t('config.editor.confirmDeleteActiveReset')
  }
  return t('config.editor.confirmDeleteBackup', { file: selectedFile.value })
})

const footerPath = computed(() => {
  const info = filesInfo.value
  return info ? `${info.config_dir}/${info.selected_file}` : ''
})

function formatProfileOption(entry: ConfigFileEntry): string {
  const title = entry.label ? `${entry.label} (${entry.file})` : entry.file
  const badge = entry.active ? ` · ${t('config.editor.activeBadge')}` : ''
  const summary = entry.summary ? ` — ${entry.summary}` : ''
  return `${title}${badge}${summary}`
}

function applyFilesInfo(info: ConfigFilesInfo): void {
  filesInfo.value = info
  savedRawText.value = info.raw_text
  rawText.value = info.raw_text
  actionError.value = null
  confirmingDelete.value = false
  creatingProfile.value = false
  pendingDirtyAction.value = null

  if (info.parse_error) {
    jsonParseError.value = info.parse_error
    doc.value = {}
    editorMode.value = 'json'
    return
  }
  jsonParseError.value = null
  doc.value = isPlainObject(info.parsed) ? cloneJson(info.parsed) : {}
}

async function fetchInitial(): Promise<void> {
  loading.value = true
  actionError.value = null
  try {
    const info = await connection.getConfigFiles(undefined, workspace.effectiveCwd)
    applyFilesInfo(info)
  } catch (error) {
    actionError.value = describeError(error)
  } finally {
    loading.value = false
  }
}

function onUpdateDoc(nextDoc: JsonObject): void {
  doc.value = nextDoc
  rawText.value = formatJson(nextDoc)
  jsonParseError.value = null
}

function onUpdateRawText(nextRaw: string): void {
  rawText.value = nextRaw
  if (jsonParseError.value !== null) {
    const check = parseJsoncObject(nextRaw)
    if (check.ok) jsonParseError.value = null
  }
}

function switchEditorMode(target: EditorMode): void {
  if (target === editorMode.value) return
  if (target === 'json') {
    editorMode.value = 'json'
    return
  }
  const parsed = parseJsoncObject(rawText.value)
  if (!parsed.ok) {
    jsonParseError.value = parsed.rootNotObject ? t('config.editor.rootMustBeObject') : parsed.error
    return
  }
  doc.value = parsed.value
  jsonParseError.value = null
  editorMode.value = 'visual'
}

function discardEdits(): void {
  const info = filesInfo.value
  if (!info) return
  rawText.value = savedRawText.value
  actionError.value = null
  if (info.parse_error) {
    jsonParseError.value = info.parse_error
    doc.value = {}
    editorMode.value = 'json'
    return
  }
  jsonParseError.value = null
  doc.value = isPlainObject(info.parsed) ? cloneJson(info.parsed) : {}
}

function guardDirty(action: PendingDirtyAction): boolean {
  if (!dirty.value) return true
  pendingDirtyAction.value = action
  confirmingDelete.value = false
  creatingProfile.value = false
  return false
}

async function confirmDiscardAndProceed(): Promise<void> {
  const action = pendingDirtyAction.value
  pendingDirtyAction.value = null
  if (!action) return
  if (action.kind === 'close') {
    dialogs.close()
    return
  }
  discardEdits()
  if (action.kind === 'select') {
    await activateProfile(action.file)
    return
  }
  if (action.kind === 'reload') {
    await reloadAll()
    return
  }
  openCreateBar()
}

function onProfileSelectChange(event: Event): void {
  const select = event.target as HTMLSelectElement
  const targetFile = select.value
  if (targetFile === selectedFile.value) return
  if (!guardDirty({ kind: 'select', file: targetFile })) {
    select.value = selectedFile.value
    return
  }
  void activateProfile(targetFile)
}

async function activateProfile(file: string): Promise<void> {
  busy.value = true
  actionError.value = null
  try {
    const info = await connection.selectConfigFile(file, workspace.effectiveCwd)
    applyFilesInfo(info)
  } catch (error) {
    actionError.value = describeError(error)
  } finally {
    busy.value = false
  }
}

function openCreateBar(): void {
  if (!guardDirty({ kind: 'openCreate' })) return
  confirmingDelete.value = false
  newProfileLabel.value = ''
  cloneCurrent.value = true
  creatingProfile.value = true
}

async function submitCreateProfile(): Promise<void> {
  busy.value = true
  actionError.value = null
  try {
    const trimmed = newProfileLabel.value.trim()
    const info = await connection.createConfigFile({
      ...(trimmed !== '' ? { label: trimmed } : {}),
      cloneCurrent: cloneCurrent.value,
      cwd: workspace.effectiveCwd,
    })
    applyFilesInfo(info)
  } catch (error) {
    actionError.value = describeError(error)
  } finally {
    busy.value = false
  }
}

function requestDeleteProfile(): void {
  if (!canDelete.value) return
  creatingProfile.value = false
  pendingDirtyAction.value = null
  confirmingDelete.value = true
}

async function confirmDeleteProfile(): Promise<void> {
  busy.value = true
  actionError.value = null
  try {
    const info = await connection.deleteConfigFile(selectedFile.value, workspace.effectiveCwd)
    applyFilesInfo(info)
  } catch (error) {
    actionError.value = describeError(error)
  } finally {
    busy.value = false
  }
}

function requestReload(): void {
  if (!guardDirty({ kind: 'reload' })) return
  void reloadAll()
}

async function reloadAll(): Promise<void> {
  busy.value = true
  actionError.value = null
  try {
    await connection.reloadConfig(workspace.effectiveCwd)
    const info = await connection.getConfigFiles(undefined, workspace.effectiveCwd)
    applyFilesInfo(info)
    notices.push('info', t('command.reload.done'))
  } catch (error) {
    actionError.value = describeError(error)
  } finally {
    busy.value = false
  }
}

async function save(): Promise<void> {
  if (!dirty.value || busy.value || !filesInfo.value) return
  const parsed = parseJsoncObject(rawText.value)
  if (!parsed.ok) {
    jsonParseError.value = parsed.rootNotObject ? t('config.editor.rootMustBeObject') : parsed.error
    return
  }
  jsonParseError.value = null
  busy.value = true
  actionError.value = null
  try {
    const info = await connection.saveConfigFile(
      filesInfo.value.selected_file,
      rawText.value,
      workspace.effectiveCwd,
    )
    applyFilesInfo(info)
    notices.push('info', t('config.editor.saved'))
  } catch (error) {
    actionError.value = describeError(error)
  } finally {
    busy.value = false
  }
}

function onWindowKeydown(event: KeyboardEvent): void {
  const isSave = (event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 's'
  if (!isSave) return
  event.preventDefault()
  void save()
}

onMounted(() => {
  dialogs.setCloseGuard(() => guardDirty({ kind: 'close' }))
  window.addEventListener('keydown', onWindowKeydown)
  void fetchInitial()
})

onBeforeUnmount(() => {
  dialogs.setCloseGuard(undefined)
  window.removeEventListener('keydown', onWindowKeydown)
})
</script>

<template>
  <section class="config-dialog" role="dialog" aria-modal="true" :aria-label="t('config.editor.title')">
    <header class="topbar">
      <div class="profile-group">
        <div class="profile-select-wrap">
          <select
            class="profile-select"
            :value="selectedFile"
            :disabled="loading || busy || files.length === 0"
            :aria-label="t('config.editor.profileLabel')"
            @change="onProfileSelectChange"
          >
            <option v-for="entry in files" :key="entry.file" :value="entry.file">
              {{ formatProfileOption(entry) }}
            </option>
          </select>
          <Chevron :size="12" class="select-chevron" />
        </div>

        <button
          type="button"
          class="tool-btn"
          :disabled="loading || busy"
          :title="t('config.editor.newProfile')"
          @click="openCreateBar"
        >
          <Icon icon="plus-16" />
          <span>{{ t('config.editor.newProfile') }}</span>
        </button>

        <button
          type="button"
          class="tool-btn icon-only danger"
          :disabled="loading || busy || !canDelete"
          :aria-label="t('config.editor.deleteProfile')"
          :title="t('config.editor.deleteProfile')"
          @click="requestDeleteProfile"
        >
          <Icon icon="trash-16" />
        </button>

        <button
          type="button"
          class="tool-btn icon-only"
          :disabled="loading || busy"
          :aria-label="t('config.editor.reload')"
          :title="t('config.editor.reload')"
          @click="requestReload"
        >
          <Icon icon="retry-20" />
        </button>
      </div>

      <div class="topbar-right">
        <Segmented
          :model-value="editorMode"
          :options="modeOptions"
          :label="t('config.editor.modeLabel')"
          @update:model-value="switchEditorMode"
        />
        <button
          type="button"
          class="close-btn"
          :aria-label="t('dialog.close')"
          @click="dialogs.requestClose()"
        >
          <Icon icon="close-20" />
        </button>
      </div>
    </header>

    <div v-if="pendingDirtyAction" class="prompt-bar warn" role="alert">
      <div class="prompt-text">
        <strong>{{ t('config.editor.unsavedPrompt') }}</strong>
        <span>{{ t('config.editor.unsavedHint') }}</span>
      </div>
      <div class="prompt-actions">
        <DialogButton small :label="t('dialog.cancel')" @click="pendingDirtyAction = null" />
        <DialogButton
          small
          kind="danger"
          :label="t('config.editor.discard')"
          @click="confirmDiscardAndProceed"
        />
      </div>
    </div>

    <div v-if="creatingProfile" class="prompt-bar neutral">
      <input
        v-model="newProfileLabel"
        type="text"
        class="inline-input"
        spellcheck="false"
        :placeholder="t('config.editor.newProfileNamePlaceholder')"
        :aria-label="t('config.editor.newProfileNamePlaceholder')"
        @keydown.enter.prevent="!$event.isComposing && submitCreateProfile()"
      />
      <label class="clone-toggle">
        <span>{{ t('config.editor.cloneCurrent') }}</span>
        <ToggleSwitch v-model="cloneCurrent" :label="t('config.editor.cloneCurrent')" />
      </label>
      <div class="prompt-actions">
        <DialogButton small :label="t('dialog.cancel')" @click="creatingProfile = false" />
        <DialogButton
          small
          kind="primary"
          :label="t('config.editor.create')"
          :disabled="busy"
          @click="submitCreateProfile"
        />
      </div>
    </div>

    <div v-if="confirmingDelete" class="prompt-bar danger" role="alert">
      <span class="prompt-text">{{ deletePromptText }}</span>
      <div class="prompt-actions">
        <DialogButton small :label="t('dialog.cancel')" @click="confirmingDelete = false" />
        <DialogButton
          small
          kind="danger"
          :label="t('config.editor.deleteProfile')"
          :disabled="busy"
          @click="confirmDeleteProfile"
        />
      </div>
    </div>

    <div v-if="!loading && filesInfo" class="banners">
      <div v-if="activeEntry && !activeEntry.exists" class="banner info">
        <Icon icon="info-16" class="banner-icon" />
        <span>{{ t('config.editor.bannerNotCreated') }}</span>
      </div>
      <div v-if="filesInfo.has_jsonc_override" class="banner warn">
        <Icon icon="warning-16" class="banner-icon" />
        <span>{{ t('config.editor.bannerJsoncOverride') }}</span>
      </div>
      <div v-if="filesInfo.env_config_override" class="banner warn">
        <Icon icon="warning-16" class="banner-icon" />
        <span>{{ t('config.editor.bannerEnvOverride', { path: filesInfo.env_config_override }) }}</span>
      </div>
      <div v-if="filesInfo.project_overrides.length > 0" class="banner info">
        <Icon icon="info-16" class="banner-icon" />
        <span>
          {{ t('config.editor.bannerProjectOverrides', { files: filesInfo.project_overrides.join(', ') }) }}
        </span>
      </div>
      <div v-for="(warning, index) in filesInfo.warnings" :key="index" class="banner warn">
        <Icon icon="warning-16" class="banner-icon" />
        <span>{{ warning }}</span>
      </div>
      <div v-if="jsonParseError" class="banner error" role="alert">
        <Icon icon="error-16" class="banner-icon" />
        <span>{{ t('config.editor.bannerParseError', { error: jsonParseError }) }}</span>
      </div>
    </div>

    <div v-if="actionError" class="banners">
      <div class="banner error" role="alert">
        <Icon icon="error-16" class="banner-icon" />
        <span>{{ actionError }}</span>
      </div>
    </div>

    <div v-if="loading" class="loading-pane">{{ t('config.editor.loading') }}</div>

    <div v-else-if="editorMode === 'visual'" class="visual-layout">
      <nav class="nav" :aria-label="t('config.editor.title')">
        <button
          v-for="item in NAV_ITEMS"
          :key="item.id"
          type="button"
          class="nav-item"
          :class="{ active: activeSection === item.id }"
          @click="activeSection = item.id"
        >
          <Icon :icon="item.icon" class="nav-icon" />
          <span>{{ t(`config.editor.nav.${item.id}`) }}</span>
        </button>
      </nav>

      <div class="pane content">
        <GeneralSection
          v-if="activeSection === 'general'"
          :doc="doc"
          :server-models="connection.config?.models ?? []"
          :server-agents="connection.config?.agents ?? []"
          @update:doc="onUpdateDoc"
        />
        <ProvidersSection
          v-else-if="activeSection === 'providers'"
          :doc="doc"
          :env-status="filesInfo?.env_status"
          @update:doc="onUpdateDoc"
        />
        <AgentsSection
          v-else-if="activeSection === 'agents'"
          :doc="doc"
          :server-models="connection.config?.models ?? []"
          @update:doc="onUpdateDoc"
        />
        <PermissionsSection
          v-else-if="activeSection === 'permissions'"
          :doc="doc"
          @update:doc="onUpdateDoc"
        />
        <McpSection
          v-else-if="activeSection === 'mcp'"
          :doc="doc"
          :env-status="filesInfo?.env_status"
          @update:doc="onUpdateDoc"
        />
        <CommandsSection
          v-else-if="activeSection === 'commands'"
          :doc="doc"
          :server-models="connection.config?.models ?? []"
          :server-agents="connection.config?.agents ?? []"
          @update:doc="onUpdateDoc"
        />
      </div>
    </div>

    <div v-else class="json-pane content">
      <TextAreaField
        monospace
        :rows="18"
        :model-value="rawText"
        :label="t('config.editor.modeJson')"
        :disabled="busy"
        @update:model-value="onUpdateRawText"
      />
    </div>

    <footer class="footer">
      <span class="file-path" :title="footerPath">{{ footerPath }}</span>
      <div class="footer-actions">
        <DialogButton
          v-if="dirty"
          :label="t('config.editor.discard')"
          :disabled="busy"
          @click="discardEdits"
        />
        <DialogButton
          kind="primary"
          :label="busy ? t('config.editor.saving') : t('config.editor.save')"
          :disabled="!dirty || busy"
          @click="save"
        />
      </div>
    </footer>
  </section>
</template>

<style scoped>
.config-dialog {
  display: flex;
  flex-direction: column;
  width: min(var(--config-dialog-width), calc(100vw - 24px));
  height: min(760px, 84vh);
  max-height: 84vh;
  overflow: hidden;
  border-radius: var(--radius-large);
  background: var(--surface);
  box-shadow:
    inset 0 0 0 1px var(--border),
    0 12px 40px #00000040;
}

.topbar {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 10px;
  min-height: var(--dialog-header-height);
  padding: 10px 14px 10px 18px;
  border-bottom: 1px solid var(--border);
}

.profile-group {
  display: flex;
  align-items: center;
  flex: 1 1 320px;
  min-width: 0;
  gap: 6px;
}

.profile-select-wrap {
  position: relative;
  display: flex;
  align-items: center;
  flex: 1;
  min-width: 140px;
  max-width: 360px;
}

.profile-select {
  appearance: none;
  width: 100%;
  height: 32px;
  padding: 0 28px 0 10px;
  border: 0;
  outline: 0;
  border-radius: var(--radius-small);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
  font-size: var(--small-font-size);
  cursor: pointer;
}

.profile-select:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.select-chevron {
  position: absolute;
  right: 9px;
  color: var(--text-muted);
  pointer-events: none;
  rotate: 90deg;
}

.tool-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  height: 32px;
  padding: 0 10px;
  border-radius: var(--radius-small);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font-size: var(--small-font-size);
  white-space: nowrap;
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.tool-btn.icon-only {
  width: 32px;
  padding: 0;
  color: var(--text-muted);
}

.tool-btn:not(:disabled):hover {
  background: var(--surface-hover);
  color: var(--text);
}

.tool-btn.danger:not(:disabled):hover {
  color: var(--danger-text);
}

.tool-btn:disabled {
  opacity: var(--disabled-opacity);
  cursor: default;
}

.topbar-right {
  display: flex;
  align-items: center;
  gap: 10px;
}

.close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 50%;
  color: var(--text-muted);
}

.close-btn:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.prompt-bar {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 10px;
  padding: 10px 18px;
  border-bottom: 1px solid var(--border);
  font-size: var(--small-font-size);
}

.prompt-bar.neutral {
  background: var(--code-background);
}

.prompt-bar.warn {
  background: var(--warning-surface);
  border-bottom-color: var(--warning-border);
  color: var(--warning-text);
}

.prompt-bar.danger {
  background: var(--error-surface);
  border-bottom-color: var(--error-border);
  color: var(--danger-text);
}

.prompt-text {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 180px;
}

.prompt-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.inline-input {
  flex: 1;
  min-width: 160px;
  height: 32px;
  padding: 0 10px;
  border: 0;
  outline: 0;
  border-radius: var(--radius-small);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
}

.clone-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--text);
}

.banners {
  display: flex;
  flex: none;
  flex-direction: column;
  gap: 6px;
  padding: 8px 18px 0;
}

.banner {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 7px 10px;
  border-radius: var(--radius-small);
  font-size: var(--small-font-size);
  line-height: 1.4;
}

.banner-icon {
  flex: none;
  margin-top: 1px;
}

.banner.info {
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
  color: var(--info-text);
}

.banner.warn {
  background: var(--warning-surface);
  box-shadow: inset 0 0 0 1px var(--warning-border);
  color: var(--warning-text);
}

.banner.error {
  background: var(--error-surface);
  box-shadow: inset 0 0 0 1px var(--error-border);
  color: var(--danger-text);
}

.loading-pane {
  display: flex;
  flex: 1;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
}

.visual-layout {
  display: flex;
  flex: 1;
  min-height: 0;
}

.nav {
  display: flex;
  flex: 0 0 200px;
  flex-direction: column;
  gap: 2px;
  padding: 12px 10px;
  border-right: 1px solid var(--border);
  overflow-y: auto;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding: 0 10px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
  font-size: var(--secondary-font-size);
  text-align: left;
  transition:
    background-color var(--motion-quick) var(--motion-ease),
    color var(--motion-quick) var(--motion-ease);
}

.nav-item:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.nav-item.active {
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
  color: var(--text);
  font-weight: 500;
}

.nav-icon {
  flex: none;
}

.pane {
  flex: 1;
  min-width: 0;
  min-height: 0;
  padding: 14px 22px 22px;
  overflow-y: auto;
}

.json-pane {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  padding: 14px 18px;
  overflow-y: auto;
}

.json-pane :deep(.textarea-field),
.json-pane :deep(.textarea) {
  flex: 1;
  min-height: 280px;
}

.footer {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 58px;
  padding: 8px 18px;
  border-top: 1px solid var(--border);
}

.file-path {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-muted);
  font-family: var(--font-mono);
  font-size: var(--small-font-size);
  user-select: text;
}

.footer-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: none;
}

@media (max-width: 680px) {
  .visual-layout {
    flex-direction: column;
  }

  .nav {
    flex: none;
    flex-direction: row;
    border-right: 0;
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
  }

  .nav-item {
    white-space: nowrap;
  }
}
</style>
