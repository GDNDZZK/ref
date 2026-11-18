<script setup lang="ts">
// Providers & Models configuration section (`provider.*`).
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  PROVIDER_PRESETS,
  getAt,
  getBooleanAt,
  getNumberAt,
  getObjectAt,
  getStringArrayAt,
  getStringAt,
  getStringMapAt,
  isPlainObject,
  setAt,
  setOwn,
  type JsonObject,
  type ProviderPreset,
  type ProviderPresetId,
} from '../../../config/jsonPatch.ts'
import {
  extractExtraProviderOptions,
  mergeExtraProviderOptions,
  normalizeProseInput,
  normalizeTrimmedString,
  renameObjectKey,
  uniqueObjectKey,
} from '../../../config/sectionHelpers.ts'
import {
  useConfigSubEditorScope,
  useInlineRenameDrafts,
  useJsonObjectDrafts,
} from '../../../config/subEditorState.ts'
import Chevron from '../../Chevron.vue'
import DialogButton from '../../DialogButton.vue'
import Icon from '../../Icon.vue'
import ToggleSwitch from '../../ToggleSwitch.vue'
import {
  FormRow,
  KeyValueEditor,
  NumberField,
  SecretField,
  SelectField,
  StringListEditor,
  TextAreaField,
} from '../index.ts'
import ProviderModelsEditor from './ProviderModelsEditor.vue'

const NPM_OPTIONS = [
  { value: '@ai-sdk/anthropic', label: '@ai-sdk/anthropic' },
  { value: '@ai-sdk/openai', label: '@ai-sdk/openai' },
  { value: '@ai-sdk/openai-compatible', label: '@ai-sdk/openai-compatible' },
] as const

const API_OPTIONS = [
  { value: 'anthropic', label: 'anthropic' },
  { value: 'responses', label: 'responses' },
  { value: 'chat', label: 'chat' },
] as const

const EXTRA_OPTIONS_PLACEHOLDER = '{ "timeout": 30000 }'
const props = defineProps<{
  doc: JsonObject
  envStatus?: Readonly<Record<string, boolean>>
}>()

const emit = defineEmits<{ 'update:doc': [next: JsonObject] }>()
const { t } = useI18n()

const adding = ref(false)
const chosenPresetId = ref<ProviderPresetId>('anthropic')
const newProviderId = ref('anthropic')
const openProviders = reactive(new Set<string>())
let lastEmittedDoc: JsonObject | undefined

const providersObj = computed(() => getObjectAt(props.doc, ['provider']) ?? {})
const providerIds = computed(() => Object.keys(providersObj.value))

function emitDoc(next: JsonObject): void {
  lastEmittedDoc = next
  emit('update:doc', next)
}

function providerObj(id: string): JsonObject {
  const raw = providersObj.value[id]
  return isPlainObject(raw) ? raw : {}
}

const optionsDrafts = useJsonObjectDrafts({
  allowEmpty: true,
  invalidMessage: () => t('config.editor.providers.variantInvalidJson'),
})

const renames = useInlineRenameDrafts({
  exists: (key) => Object.hasOwn(providersObj.value, key),
  collisionMessage: () => t('config.editor.renameExists'),
  onCommit(oldId, target) {
    const renamed = renameObjectKey(providersObj.value, oldId, target)
    if (openProviders.has(oldId)) {
      openProviders.delete(oldId)
      openProviders.add(target)
    }
    optionsDrafts.renameKey(oldId, target)
    emitDoc(setAt(props.doc, ['provider'], renamed))
  },
})

const subEditorScope = useConfigSubEditorScope(
  () => ({
    dirty: optionsDrafts.hasInvalid.value || renames.hasDirty.value,
    committable: renames.hasCommittable.value,
    invalid: optionsDrafts.hasInvalid.value,
  }),
  () => renames.flushAll(),
)

watch(
  () => props.doc,
  (nextDoc) => {
    if (nextDoc === lastEmittedDoc) return
    const forceClear = !subEditorScope.hasHost
    optionsDrafts.reconcile((id) => extractExtraProviderOptions(providerObj(id)), forceClear)
    renames.reconcileKeys(providerIds.value, forceClear)
  },
)

if (subEditorScope.resetVersion) {
  watch(subEditorScope.resetVersion, () => {
    optionsDrafts.clearAll()
    renames.clearAll()
  })
}

const presetOptions = computed(() =>
  PROVIDER_PRESETS.map((preset) => ({ value: preset.id, label: preset.label })),
)

function startAddProvider(): void {
  adding.value = true
  selectPreset('anthropic')
}

function selectPreset(id: string): void {
  const found = PROVIDER_PRESETS.find((preset) => preset.id === id) ?? PROVIDER_PRESETS[0]!
  chosenPresetId.value = found.id
  const baseId = found.providerId || 'custom-provider'
  newProviderId.value = uniqueObjectKey(providersObj.value, baseId)
}

function buildPresetEntry(preset: ProviderPreset): JsonObject {
  const entry: JsonObject = {}
  if (preset.id === 'custom') {
    setOwn(entry, 'npm', preset.npm)
  } else if (preset.id !== 'anthropic' && preset.id !== 'openai') {
    setOwn(entry, 'name', preset.label)
    setOwn(entry, 'npm', preset.npm)
  }
  const options: JsonObject = {}
  if (preset.id === 'ollama' || preset.id === 'custom') {
    setOwn(options, 'baseURL', preset.defaultBaseUrl)
  }
  if (preset.apiKeyPlaceholder) {
    setOwn(options, 'apiKey', preset.apiKeyPlaceholder)
  }
  if (Object.keys(options).length > 0) setOwn(entry, 'options', options)
  return entry
}

function confirmAddProvider(): void {
  const id = newProviderId.value.trim()
  if (id === '' || Object.hasOwn(providersObj.value, id)) return
  const preset = PROVIDER_PRESETS.find((entry) => entry.id === chosenPresetId.value) ?? PROVIDER_PRESETS[0]!
  const created = buildPresetEntry(preset)
  openProviders.add(id)
  adding.value = false
  emitDoc(setAt(props.doc, ['provider', id], created))
}

function toggleProvider(id: string): void {
  if (openProviders.has(id)) openProviders.delete(id)
  else openProviders.add(id)
}

function removeProvider(id: string): void {
  openProviders.delete(id)
  renames.remove(id)
  optionsDrafts.remove(id)
  emitDoc(setAt(props.doc, ['provider', id], undefined))
}

function patchProvider(id: string, subPath: readonly string[], value: unknown): void {
  const nextProvider = setAt(providerObj(id), subPath, value)
  emitDoc(setAt(props.doc, ['provider', id], nextProvider))
}

function patchProviderProse(id: string, subPath: readonly string[], value: string): void {
  patchProvider(id, subPath, normalizeProseInput(value))
}

function patchProviderString(id: string, subPath: readonly string[], value: string): void {
  patchProvider(id, subPath, normalizeTrimmedString(value))
}

function readBaseUrl(id: string): string {
  const p = providerObj(id)
  return (
    getStringAt(p, ['options', 'baseURL']) ??
    getStringAt(p, ['options', 'baseUrl']) ??
    getStringAt(p, ['options', 'base_url']) ??
    ''
  )
}

function writeBaseUrl(id: string, value: string): void {
  const trimmed = value.trim()
  let next = setAt(providerObj(id), ['options', 'baseUrl'], undefined)
  next = setAt(next, ['options', 'base_url'], undefined)
  next = setAt(next, ['options', 'baseURL'], trimmed === '' ? undefined : trimmed)
  emitDoc(setAt(props.doc, ['provider', id], next))
}

function readApiKey(id: string): string {
  const p = providerObj(id)
  return getStringAt(p, ['options', 'apiKey']) ?? getStringAt(p, ['options', 'api_key']) ?? ''
}

function writeApiKey(id: string, value: string): void {
  const trimmed = value.trim()
  let next = setAt(providerObj(id), ['options', 'api_key'], undefined)
  next = setAt(next, ['options', 'apiKey'], trimmed === '' ? undefined : trimmed)
  emitDoc(setAt(props.doc, ['provider', id], next))
}

function readExtraOptionsText(id: string): string {
  return optionsDrafts.read(id, extractExtraProviderOptions(providerObj(id)))
}

function canResetExtraOptions(id: string): boolean {
  return extractExtraProviderOptions(providerObj(id)) !== undefined || optionsDrafts.hasDraft(id)
}

function updateExtraOptionsText(id: string, text: string): void {
  const currentExtra = extractExtraProviderOptions(providerObj(id))
  optionsDrafts.update(id, text, currentExtra, (parsed) => {
    patchProvider(id, ['options'], mergeExtraProviderOptions(providerObj(id), parsed))
  })
}

function resetExtraOptions(id: string): void {
  optionsDrafts.remove(id)
  patchProvider(id, ['options'], mergeExtraProviderOptions(providerObj(id), undefined))
}

function patchHeaders(id: string, headers: Record<string, string>): void {
  patchProvider(id, ['options', 'headers'], Object.keys(headers).length > 0 ? headers : undefined)
}

function patchList(id: string, key: string, list: readonly string[]): void {
  const cleaned = list.filter((item) => item.trim() !== '')
  patchProvider(id, [key], cleaned.length > 0 ? cleaned : undefined)
}

function apiKeyPlaceholderFor(id: string): string {
  const preset = PROVIDER_PRESETS.find((entry) => entry.providerId === id)
  return preset?.apiKeyPlaceholder || '{env:API_KEY}'
}

function baseUrlPlaceholderFor(id: string): string {
  const preset = PROVIDER_PRESETS.find((entry) => entry.providerId === id)
  return preset?.defaultBaseUrl || 'https://api.example.com/v1'
}
</script>

<template>
  <div class="section-body">
    <div class="top-actions">
      <DialogButton
        v-if="!adding"
        kind="primary"
        small
        :label="`+ ${t('config.editor.providers.addProvider')}`"
        @click="startAddProvider"
      />
    </div>

    <div v-if="adding" class="add-panel">
      <FormRow :label="t('config.editor.providers.presetLabel')">
        <SelectField
          :model-value="chosenPresetId"
          :options="presetOptions"
          :label="t('config.editor.providers.presetLabel')"
          @update:model-value="selectPreset"
        />
      </FormRow>
      <FormRow :label="t('config.editor.providers.providerIdLabel')">
        <input
          v-model="newProviderId"
          type="text"
          class="text-input mono"
          spellcheck="false"
          :placeholder="t('config.editor.providers.providerIdPlaceholder')"
          :aria-label="t('config.editor.providers.providerIdLabel')"
          @keydown.enter.prevent="!$event.isComposing && confirmAddProvider()"
        />
      </FormRow>
      <div class="add-panel-footer">
        <DialogButton small :label="t('dialog.cancel')" @click="adding = false" />
        <DialogButton
          small
          kind="primary"
          :label="t('config.editor.create')"
          :disabled="newProviderId.trim() === '' || Object.hasOwn(providersObj, newProviderId.trim())"
          @click="confirmAddProvider"
        />
      </div>
    </div>

    <p v-if="providerIds.length === 0 && !adding" class="empty">{{ t('config.editor.providers.empty') }}</p>

    <div v-for="id in providerIds" :key="id" class="card">
      <div class="card-head">
        <button type="button" class="expand-btn" @click="toggleProvider(id)">
          <Chevron :open="openProviders.has(id)" :size="12" />
          <span class="card-id">{{ id }}</span>
          <span v-if="getStringAt(providerObj(id), ['name'])" class="card-sub">
            {{ getStringAt(providerObj(id), ['name']) }}
          </span>
        </button>
        <button
          type="button"
          class="icon-btn danger"
          :aria-label="t('config.editor.delete')"
          :title="t('config.editor.delete')"
          @click="removeProvider(id)"
        >
          <Icon icon="trash-16" />
        </button>
      </div>

      <div v-if="openProviders.has(id)" class="card-body">
        <FormRow :label="t('config.editor.providers.providerIdLabel')" :error="renames.errorOf(id)">
          <div class="rename-row">
            <input
              :value="renames.read(id)"
              type="text"
              class="text-input mono"
              spellcheck="false"
              @input="renames.set(id, ($event.target as HTMLInputElement).value)"
              @blur="renames.blur(id)"
              @keydown.enter.prevent="!$event.isComposing && renames.commit(id)"
            />
            <DialogButton
              small
              :label="t('config.editor.rename')"
              :disabled="!renames.canCommit(id)"
              @mousedown.prevent
              @click="renames.commit(id)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.providers.displayName')">
          <input
            :value="getStringAt(providerObj(id), ['name']) ?? ''"
            type="text"
            class="text-input"
            spellcheck="false"
            :placeholder="id"
            @input="patchProviderProse(id, ['name'], ($event.target as HTMLInputElement).value)"
            @blur="patchProviderString(id, ['name'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.providers.npmPackage')"
          :can-reset="getAt(providerObj(id), ['npm']) !== undefined"
          @reset="patchProvider(id, ['npm'], undefined)"
        >
          <SelectField
            monospace
            :model-value="getStringAt(providerObj(id), ['npm']) ?? ''"
            :options="NPM_OPTIONS"
            :placeholder="t('config.editor.triDefault')"
            :label="t('config.editor.providers.npmPackage')"
            @update:model-value="patchProviderString(id, ['npm'], $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.providers.apiKind')"
          :can-reset="getAt(providerObj(id), ['api']) !== undefined"
          @reset="patchProvider(id, ['api'], undefined)"
        >
          <SelectField
            monospace
            :model-value="getStringAt(providerObj(id), ['api']) ?? ''"
            :options="API_OPTIONS"
            :placeholder="t('config.editor.triDefault')"
            :label="t('config.editor.providers.apiKind')"
            @update:model-value="patchProviderString(id, ['api'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.baseUrl')">
          <input
            :value="readBaseUrl(id)"
            type="text"
            class="text-input mono"
            spellcheck="false"
            :placeholder="baseUrlPlaceholderFor(id)"
            :aria-label="t('config.editor.providers.baseUrl')"
            @input="writeBaseUrl(id, ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.providers.apiKey')"
          :description="t('config.editor.providers.apiKeyDesc')"
        >
          <SecretField
            :model-value="readApiKey(id)"
            :env-status="envStatus"
            :placeholder="apiKeyPlaceholderFor(id)"
            :label="t('config.editor.providers.apiKey')"
            @update:model-value="writeApiKey(id, $event)"
          />
        </FormRow>

        <FormRow
          vertical
          :label="t('config.editor.providers.envVars')"
          :description="t('config.editor.providers.envVarsDesc')"
        >
          <StringListEditor
            monospace
            :model-value="getStringArrayAt(providerObj(id), ['env'])"
            placeholder="OPENAI_API_KEY"
            @update:model-value="patchList(id, 'env', $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.providers.setCacheKey')"
          :can-reset="getAt(providerObj(id), ['options', 'setCacheKey']) !== undefined"
          @reset="patchProvider(id, ['options', 'setCacheKey'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(providerObj(id), ['options', 'setCacheKey']) ?? false"
              :label="t('config.editor.providers.setCacheKey')"
              @update:model-value="patchProvider(id, ['options', 'setCacheKey'], $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.providers.chunkTimeout')">
          <NumberField
            :model-value="getNumberAt(providerObj(id), ['options', 'chunkTimeout'])"
            :min="0"
            :step="1000"
            placeholder="120000"
            @update:model-value="patchProvider(id, ['options', 'chunkTimeout'], $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.providers.headers')">
          <KeyValueEditor
            :model-value="getStringMapAt(providerObj(id), ['options', 'headers'])"
            :env-status="envStatus"
            @update:model-value="patchHeaders(id, $event)"
          />
        </FormRow>

        <FormRow
          vertical
          :label="t('config.editor.providers.extraOptions')"
          :description="t('config.editor.providers.extraOptionsDesc')"
          :can-reset="canResetExtraOptions(id)"
          @reset="resetExtraOptions(id)"
        >
          <TextAreaField
            monospace
            :rows="3"
            :model-value="readExtraOptionsText(id)"
            :error="optionsDrafts.errorOf(id)"
            :placeholder="EXTRA_OPTIONS_PLACEHOLDER"
            :label="t('config.editor.providers.extraOptions')"
            @update:model-value="updateExtraOptionsText(id, $event)"
            @blur="optionsDrafts.blur(id)"
          />
        </FormRow>
        <FormRow vertical :label="t('config.editor.providers.whitelist')">
          <StringListEditor
            monospace
            :model-value="getStringArrayAt(providerObj(id), ['whitelist'])"
            @update:model-value="patchList(id, 'whitelist', $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.providers.blacklist')">
          <StringListEditor
            monospace
            :model-value="getStringArrayAt(providerObj(id), ['blacklist'])"
            @update:model-value="patchList(id, 'blacklist', $event)"
          />
        </FormRow>

        <ProviderModelsEditor
          :doc="doc"
          :provider-id="id"
          :env-status="envStatus"
          @update:doc="emitDoc($event)"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.section-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.top-actions {
  display: flex;
  justify-content: flex-start;
}

.add-panel {
  padding: 12px 14px;
  border-radius: var(--radius);
  background: var(--code-background);
  box-shadow: inset 0 0 0 1px var(--border);
}

.add-panel-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 8px;
}

.empty {
  margin: 8px 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.card {
  border-radius: var(--radius);
  background: var(--code-background);
  box-shadow: inset 0 0 0 1px var(--border);
  overflow: hidden;
}

.card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
}

.expand-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  text-align: left;
}

.card-id {
  font-family: var(--font-mono);
  font-size: var(--font-size);
  font-weight: 600;
}

.card-sub {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.card-body {
  padding: 4px 16px 16px;
  border-top: 1px solid var(--border);
}

.rename-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.text-input {
  width: 100%;
  height: var(--field-height);
  padding: 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font: inherit;
}

.text-input.mono {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
}

.text-input:focus {
  box-shadow: inset 0 0 0 1px var(--accent-text);
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
}

.icon-btn.danger:hover {
  background: var(--surface-hover);
  color: var(--danger-text);
}

.switch-wrap {
  display: flex;
  justify-content: flex-end;
}
</style>
