<script setup lang="ts">
// Nested Models & Variants CRUD under `provider.<providerId>.models`.
import { computed, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { parseJsoncObject } from '../../../config/jsonc.ts'
import {
  PROVIDER_PRESETS,
  formatJson,
  getAt,
  getBooleanAt,
  getNumberAt,
  getObjectAt,
  getStringAt,
  getStringMapAt,
  isPlainObject,
  setAt,
  type JsonObject,
} from '../../../config/jsonPatch.ts'
import Chevron from '../../Chevron.vue'
import DialogButton from '../../DialogButton.vue'
import Icon from '../../Icon.vue'
import ToggleSwitch from '../../ToggleSwitch.vue'
import { FormRow, KeyValueEditor, NumberField, TextAreaField } from '../index.ts'

const props = defineProps<{
  doc: JsonObject
  providerId: string
  envStatus?: Readonly<Record<string, boolean>>
}>()

const emit = defineEmits<{ 'update:doc': [next: JsonObject] }>()
const { t } = useI18n()

const newModelKey = ref('')
const openModels = reactive(new Set<string>())
const newVariantNames = reactive<Record<string, string>>({})
const variantDrafts = reactive<Record<string, string>>({})
const variantErrors = reactive<Record<string, string>>({})

const modelsObj = computed(() => getObjectAt(props.doc, ['provider', props.providerId, 'models']) ?? {})
const modelKeys = computed(() => Object.keys(modelsObj.value))

const suggestedModels = computed(() => {
  const preset = PROVIDER_PRESETS.find((entry) => entry.providerId === props.providerId)
  if (!preset) return []
  return preset.suggestedModels.filter((modelId) => !Object.hasOwn(modelsObj.value, modelId))
})

function patchModel(modelKey: string, subPath: readonly string[], value: unknown): void {
  emit('update:doc', setAt(props.doc, ['provider', props.providerId, 'models', modelKey, ...subPath], value))
}

function patchModelString(modelKey: string, subPath: readonly string[], value: string): void {
  const trimmed = value.trim()
  patchModel(modelKey, subPath, trimmed === '' ? undefined : trimmed)
}

function patchModelHeaders(modelKey: string, headers: Record<string, string>): void {
  patchModel(modelKey, ['headers'], Object.keys(headers).length > 0 ? headers : undefined)
}

function addModelByName(rawKey: string): void {
  const trimmed = rawKey.trim()
  if (trimmed === '' || Object.hasOwn(modelsObj.value, trimmed)) return
  openModels.add(trimmed)
  emit('update:doc', setAt(props.doc, ['provider', props.providerId, 'models', trimmed], {}))
  newModelKey.value = ''
}

function removeModel(modelKey: string): void {
  openModels.delete(modelKey)
  emit('update:doc', setAt(props.doc, ['provider', props.providerId, 'models', modelKey], undefined))
}

function toggleModel(modelKey: string): void {
  if (openModels.has(modelKey)) openModels.delete(modelKey)
  else openModels.add(modelKey)
}

function variantsOf(modelKey: string): string[] {
  const raw = getObjectAt(props.doc, ['provider', props.providerId, 'models', modelKey, 'variants'])
  return raw ? Object.keys(raw) : []
}

function variantDraftKey(modelKey: string, variantName: string): string {
  return `${modelKey}::${variantName}`
}

function readVariantText(modelKey: string, variantName: string): string {
  const key = variantDraftKey(modelKey, variantName)
  if (Object.hasOwn(variantDrafts, key)) return variantDrafts[key]!
  const obj = getObjectAt(props.doc, [
    'provider',
    props.providerId,
    'models',
    modelKey,
    'variants',
    variantName,
  ])
  return formatJson(obj ?? {}).trimEnd()
}

function updateVariantText(modelKey: string, variantName: string, text: string): void {
  const key = variantDraftKey(modelKey, variantName)
  variantDrafts[key] = text
  const parsed = parseJsoncObject(text)
  if (!parsed.ok) {
    variantErrors[key] = t('config.editor.providers.variantInvalidJson')
    return
  }
  delete variantErrors[key]
  patchModel(modelKey, ['variants', variantName], parsed.value)
}

function addVariant(modelKey: string): void {
  const rawName = (newVariantNames[modelKey] ?? '').trim()
  if (rawName === '') return
  const existing = getObjectAt(props.doc, ['provider', props.providerId, 'models', modelKey, 'variants'])
  if (existing && Object.hasOwn(existing, rawName)) return
  newVariantNames[modelKey] = ''
  patchModel(modelKey, ['variants', rawName], {})
}

function removeVariant(modelKey: string, variantName: string): void {
  const key = variantDraftKey(modelKey, variantName)
  delete variantDrafts[key]
  delete variantErrors[key]
  patchModel(modelKey, ['variants', variantName], undefined)
}

function modelHasImage(modelKey: string): boolean {
  const path = ['provider', props.providerId, 'models', modelKey]
  if (getBooleanAt(props.doc, [...path, 'attachment']) === true) return true
  const inputModalities = getAt(props.doc, [...path, 'modalities', 'input'])
  return Array.isArray(inputModalities) && inputModalities.includes('image')
}

function modelObject(modelKey: string): JsonObject {
  const raw = modelsObj.value[modelKey]
  return isPlainObject(raw) ? raw : {}
}
</script>

<template>
  <div class="models-editor">
    <div class="models-header">
      <h4 class="models-title">{{ t('config.editor.providers.modelsHeading') }}</h4>
      <div v-if="suggestedModels.length > 0" class="suggestions">
        <span class="suggest-label">{{ t('config.editor.providers.suggestedModels') }}</span>
        <button
          v-for="suggestion in suggestedModels"
          :key="suggestion"
          type="button"
          class="chip-btn"
          @click="addModelByName(suggestion)"
        >
          + {{ suggestion }}
        </button>
      </div>
    </div>

    <p v-if="modelKeys.length === 0" class="empty">{{ t('config.editor.providers.modelsEmpty') }}</p>

    <div v-for="modelKey in modelKeys" :key="modelKey" class="model-card">
      <div class="model-card-head">
        <button type="button" class="expand-btn" @click="toggleModel(modelKey)">
          <Chevron :open="openModels.has(modelKey)" :size="12" />
          <span class="model-key">{{ modelKey }}</span>
          <span v-if="getStringAt(modelObject(modelKey), ['name'])" class="model-sub">
            {{ getStringAt(modelObject(modelKey), ['name']) }}
          </span>
        </button>
        <button
          type="button"
          class="delete-btn"
          :aria-label="t('config.editor.delete')"
          :title="t('config.editor.delete')"
          @click="removeModel(modelKey)"
        >
          <Icon icon="trash-16" />
        </button>
      </div>

      <div v-if="openModels.has(modelKey)" class="model-card-body">
        <FormRow :label="t('config.editor.providers.displayName')">
          <input
            :value="getStringAt(modelObject(modelKey), ['name']) ?? ''"
            type="text"
            class="text-input"
            spellcheck="false"
            :placeholder="modelKey"
            @input="patchModelString(modelKey, ['name'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.wireId')">
          <input
            :value="getStringAt(modelObject(modelKey), ['id']) ?? ''"
            type="text"
            class="text-input mono"
            spellcheck="false"
            :placeholder="modelKey"
            @input="patchModelString(modelKey, ['id'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.providers.modelDisabled')"
          :can-reset="getAt(modelObject(modelKey), ['disabled']) !== undefined"
          @reset="patchModel(modelKey, ['disabled'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(modelObject(modelKey), ['disabled']) ?? false"
              :label="t('config.editor.providers.modelDisabled')"
              @update:model-value="patchModel(modelKey, ['disabled'], $event ? true : undefined)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.providers.attachment')"
          :can-reset="getAt(modelObject(modelKey), ['attachment']) !== undefined"
          @reset="patchModel(modelKey, ['attachment'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="modelHasImage(modelKey)"
              :label="t('config.editor.providers.attachment')"
              @update:model-value="patchModel(modelKey, ['attachment'], $event)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.providers.reasoning')"
          :can-reset="getAt(modelObject(modelKey), ['reasoning']) !== undefined"
          @reset="patchModel(modelKey, ['reasoning'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="
                (getBooleanAt(modelObject(modelKey), ['reasoning']) ||
                  getBooleanAt(modelObject(modelKey), ['thinking'])) ??
                false
              "
              :label="t('config.editor.providers.reasoning')"
              @update:model-value="patchModel(modelKey, ['reasoning'], $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.providers.contextLimit')">
          <NumberField
            :model-value="getNumberAt(modelObject(modelKey), ['limit', 'context'])"
            :min="0"
            :step="1024"
            placeholder="200000"
            @update:model-value="patchModel(modelKey, ['limit', 'context'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.outputLimit')">
          <NumberField
            :model-value="getNumberAt(modelObject(modelKey), ['limit', 'output'])"
            :min="0"
            :step="1024"
            placeholder="64000"
            @update:model-value="patchModel(modelKey, ['limit', 'output'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.compactLimit')">
          <NumberField
            :model-value="getNumberAt(modelObject(modelKey), ['limit', 'compact'])"
            :min="0"
            :step="1024"
            @update:model-value="patchModel(modelKey, ['limit', 'compact'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.thresholdLimit')">
          <NumberField
            :model-value="getNumberAt(modelObject(modelKey), ['limit', 'threshold'])"
            :min="0.01"
            :step="0.05"
            placeholder="0.9"
            @update:model-value="patchModel(modelKey, ['limit', 'threshold'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.temperature')">
          <NumberField
            :model-value="getNumberAt(modelObject(modelKey), ['temperature'])"
            :min="0"
            :max="2"
            :step="0.1"
            @update:model-value="patchModel(modelKey, ['temperature'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.topP')">
          <NumberField
            :model-value="getNumberAt(modelObject(modelKey), ['top_p'])"
            :min="0"
            :max="1"
            :step="0.05"
            @update:model-value="patchModel(modelKey, ['top_p'], $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.providers.headers')">
          <KeyValueEditor
            :model-value="getStringMapAt(modelObject(modelKey), ['headers'])"
            :env-status="envStatus"
            @update:model-value="patchModelHeaders(modelKey, $event)"
          />
        </FormRow>

        <div class="variants-block">
          <span class="variants-title">{{ t('config.editor.providers.variantsHeading') }}</span>
          <div v-for="variantName in variantsOf(modelKey)" :key="variantName" class="variant-card">
            <div class="variant-head">
              <span class="mono-tag">{{ variantName }}</span>
              <button
                type="button"
                class="delete-btn"
                :aria-label="t('config.editor.delete')"
                @click="removeVariant(modelKey, variantName)"
              >
                <Icon icon="close-16" />
              </button>
            </div>
            <TextAreaField
              monospace
              :rows="2"
              :model-value="readVariantText(modelKey, variantName)"
              :error="variantErrors[variantDraftKey(modelKey, variantName)]"
              :label="t('config.editor.providers.variantOptions')"
              @update:model-value="updateVariantText(modelKey, variantName, $event)"
            />
          </div>
          <div class="add-inline">
            <input
              :value="newVariantNames[modelKey] ?? ''"
              type="text"
              class="text-input mono"
              spellcheck="false"
              :placeholder="t('config.editor.providers.variantNamePlaceholder')"
              @input="newVariantNames[modelKey] = ($event.target as HTMLInputElement).value"
              @keydown.enter.prevent="!$event.isComposing && addVariant(modelKey)"
            />
            <DialogButton
              small
              :label="t('config.editor.providers.addVariant')"
              :disabled="!(newVariantNames[modelKey] ?? '').trim()"
              @click="addVariant(modelKey)"
            />
          </div>
        </div>
      </div>
    </div>

    <div class="add-inline">
      <input
        v-model="newModelKey"
        type="text"
        class="text-input mono"
        spellcheck="false"
        :placeholder="t('config.editor.providers.modelKeyPlaceholder')"
        :aria-label="t('config.editor.providers.modelKeyPlaceholder')"
        @keydown.enter.prevent="!$event.isComposing && addModelByName(newModelKey)"
      />
      <DialogButton
        small
        :label="t('config.editor.providers.addModel')"
        :disabled="newModelKey.trim() === ''"
        @click="addModelByName(newModelKey)"
      />
    </div>
  </div>
</template>

<style scoped>
.models-editor {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
}

.models-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px;
}

.models-title {
  margin: 0;
  font-size: var(--secondary-font-size);
  font-weight: 600;
}

.suggestions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.suggest-label {
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.chip-btn {
  height: 24px;
  padding: 0 10px;
  border-radius: 12px;
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: var(--small-font-size);
  transition: background-color var(--motion-quick) var(--motion-ease);
}

.chip-btn:hover {
  background: var(--surface-hover);
}

.empty {
  margin: 0;
  color: var(--text-muted);
  font-size: var(--small-font-size);
}

.model-card {
  border-radius: var(--radius);
  background: var(--background);
  box-shadow: inset 0 0 0 1px var(--border);
  overflow: hidden;
}

.model-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
}

.expand-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  text-align: left;
}

.model-key {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
  font-weight: 600;
}

.model-sub {
  color: var(--text-muted);
  font-size: var(--small-font-size);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-card-body {
  padding: 4px 14px 14px;
  border-top: 1px solid var(--border);
}

.delete-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-small);
  color: var(--text-muted);
}

.delete-btn:hover {
  background: var(--surface-hover);
  color: var(--danger-text);
}

.text-input {
  width: 100%;
  height: var(--field-height);
  padding: 0 12px;
  border: 0;
  outline: 0;
  border-radius: var(--radius);
  background: var(--surface);
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

.switch-wrap {
  display: flex;
  justify-content: flex-end;
}

.variants-block {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--border);
}

.variants-title {
  font-size: var(--small-font-size);
  font-weight: 600;
  color: var(--text-muted);
}

.variant-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 10px;
  border-radius: var(--radius-small);
  background: var(--surface);
  box-shadow: inset 0 0 0 1px var(--border);
}

.variant-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.mono-tag {
  font-family: var(--font-mono);
  font-size: var(--code-font-size);
  font-weight: 600;
}

.add-inline {
  display: flex;
  align-items: center;
  gap: 8px;
}

.add-inline .text-input {
  flex: 1;
}
</style>
