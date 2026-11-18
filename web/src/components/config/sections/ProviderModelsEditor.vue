<script setup lang="ts">
// Nested Models & Variants CRUD under `provider.<providerId>.models`.
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  PROVIDER_PRESETS,
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
import { normalizeProseInput, normalizeTrimmedString } from '../../../config/sectionHelpers.ts'
import { useConfigSubEditorScope, useJsonObjectDrafts } from '../../../config/subEditorState.ts'
import Chevron from '../../Chevron.vue'
import DialogButton from '../../DialogButton.vue'
import Icon from '../../Icon.vue'
import ToggleSwitch from '../../ToggleSwitch.vue'
import { FormRow, KeyValueEditor, NumberField, TextAreaField } from '../index.ts'

const MODEL_OPTIONS_PLACEHOLDER = '{ "reasoningEffort": "high" }'
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
let lastEmittedDoc: JsonObject | undefined

const modelsObj = computed(() => getObjectAt(props.doc, ['provider', props.providerId, 'models']) ?? {})
const modelKeys = computed(() => Object.keys(modelsObj.value))

const suggestedModels = computed(() => {
  const preset = PROVIDER_PRESETS.find((entry) => entry.providerId === props.providerId)
  if (!preset) return []
  return preset.suggestedModels.filter((modelId) => !Object.hasOwn(modelsObj.value, modelId))
})

function emitDoc(next: JsonObject): void {
  lastEmittedDoc = next
  emit('update:doc', next)
}

function modelObject(modelKey: string): JsonObject {
  const raw = modelsObj.value[modelKey]
  return isPlainObject(raw) ? raw : {}
}

function commitModel(modelKey: string, nextModel: JsonObject): void {
  emitDoc(setAt(props.doc, ['provider', props.providerId, 'models', modelKey], nextModel))
}

function patchModel(modelKey: string, subPath: readonly string[], value: unknown): void {
  commitModel(modelKey, setAt(modelObject(modelKey), subPath, value))
}

function patchModelProse(modelKey: string, subPath: readonly string[], value: string): void {
  patchModel(modelKey, subPath, normalizeProseInput(value))
}

function patchModelString(modelKey: string, subPath: readonly string[], value: string): void {
  patchModel(modelKey, subPath, normalizeTrimmedString(value))
}

function patchModelHeaders(modelKey: string, headers: Record<string, string>): void {
  patchModel(modelKey, ['headers'], Object.keys(headers).length > 0 ? headers : undefined)
}

const optionsDrafts = useJsonObjectDrafts({
  allowEmpty: true,
  invalidMessage: () => t('config.editor.providers.variantInvalidJson'),
})

const variantDrafts = useJsonObjectDrafts({
  allowEmpty: false,
  invalidMessage: () => t('config.editor.providers.variantInvalidJson'),
})

const hasSubEditorError = computed(() => optionsDrafts.hasInvalid.value || variantDrafts.hasInvalid.value)
const subEditorScope = useConfigSubEditorScope(() => ({
  dirty: hasSubEditorError.value,
  committable: false,
  invalid: hasSubEditorError.value,
}))

function variantObjByDraftKey(draftKey: string): JsonObject | undefined {
  const sep = draftKey.indexOf('::')
  if (sep < 0) return undefined
  const modelKey = draftKey.slice(0, sep)
  const variantName = draftKey.slice(sep + 2)
  return getObjectAt(modelObject(modelKey), ['variants', variantName])
}

watch(
  () => props.doc,
  (nextDoc) => {
    if (nextDoc === lastEmittedDoc) return
    const forceClear = !subEditorScope.hasHost
    optionsDrafts.reconcile((modelKey) => getObjectAt(modelObject(modelKey), ['options']), forceClear)
    variantDrafts.reconcile(variantObjByDraftKey, forceClear)
  },
)

if (subEditorScope.resetVersion) {
  watch(subEditorScope.resetVersion, () => {
    optionsDrafts.clearAll()
    variantDrafts.clearAll()
  })
}

function addModelByName(rawKey: string): void {
  const trimmed = rawKey.trim()
  if (trimmed === '' || Object.hasOwn(modelsObj.value, trimmed)) return
  openModels.add(trimmed)
  commitModel(trimmed, {})
  newModelKey.value = ''
}

function removeModel(modelKey: string): void {
  openModels.delete(modelKey)
  optionsDrafts.remove(modelKey)
  variantDrafts.removeByPrefix(`${modelKey}::`)
  delete newVariantNames[modelKey]
  emitDoc(setAt(props.doc, ['provider', props.providerId, 'models', modelKey], undefined))
}

function toggleModel(modelKey: string): void {
  if (openModels.has(modelKey)) openModels.delete(modelKey)
  else openModels.add(modelKey)
}

function readModelOptionsText(modelKey: string): string {
  return optionsDrafts.read(modelKey, getObjectAt(modelObject(modelKey), ['options']))
}

function canResetModelOptions(modelKey: string): boolean {
  return getAt(modelObject(modelKey), ['options']) !== undefined || optionsDrafts.hasDraft(modelKey)
}

function updateModelOptionsText(modelKey: string, text: string): void {
  const currentOptions = getObjectAt(modelObject(modelKey), ['options'])
  optionsDrafts.update(modelKey, text, currentOptions, (parsed) => {
    patchModel(modelKey, ['options'], parsed)
  })
}

function resetModelOptions(modelKey: string): void {
  optionsDrafts.remove(modelKey)
  patchModel(modelKey, ['options'], undefined)
}

function variantsOf(modelKey: string): string[] {
  const raw = getObjectAt(modelObject(modelKey), ['variants'])
  return raw ? Object.keys(raw) : []
}

function variantDraftKey(modelKey: string, variantName: string): string {
  return `${modelKey}::${variantName}`
}

function readVariantText(modelKey: string, variantName: string): string {
  const key = variantDraftKey(modelKey, variantName)
  const obj = getObjectAt(modelObject(modelKey), ['variants', variantName]) ?? {}
  return variantDrafts.read(key, obj, '{}')
}

function updateVariantText(modelKey: string, variantName: string, text: string): void {
  const key = variantDraftKey(modelKey, variantName)
  const currentVariant = getObjectAt(modelObject(modelKey), ['variants', variantName]) ?? {}
  variantDrafts.update(key, text, currentVariant, (parsed) => {
    patchModel(modelKey, ['variants', variantName], parsed ?? {})
  })
}

function addVariant(modelKey: string): void {
  const rawName = (newVariantNames[modelKey] ?? '').trim()
  if (rawName === '') return
  const existing = getObjectAt(modelObject(modelKey), ['variants'])
  if (existing && Object.hasOwn(existing, rawName)) return
  newVariantNames[modelKey] = ''
  patchModel(modelKey, ['variants', rawName], {})
}

function removeVariant(modelKey: string, variantName: string): void {
  variantDrafts.remove(variantDraftKey(modelKey, variantName))
  patchModel(modelKey, ['variants', variantName], undefined)
}

function modelHasImage(modelKey: string): boolean {
  const model = modelObject(modelKey)
  if (getBooleanAt(model, ['attachment']) === true) return true
  const inputModalities = getAt(model, ['modalities', 'input'])
  return Array.isArray(inputModalities) && inputModalities.includes('image')
}

function writeModelAttachment(modelKey: string, enabled: boolean): void {
  let next = setAt(modelObject(modelKey), ['attachment'], enabled)
  if (!enabled) {
    const rawInput = getAt(next, ['modalities', 'input'])
    if (Array.isArray(rawInput) && rawInput.includes('image')) {
      const remaining = rawInput.filter((item) => item !== 'image')
      next = setAt(next, ['modalities', 'input'], remaining.length > 0 ? remaining : undefined)
    }
  }
  commitModel(modelKey, next)
}

function modelHasReasoning(modelKey: string): boolean {
  const model = modelObject(modelKey)
  return getBooleanAt(model, ['reasoning']) === true || getBooleanAt(model, ['thinking']) === true
}

function canResetReasoning(modelKey: string): boolean {
  const model = modelObject(modelKey)
  return getAt(model, ['reasoning']) !== undefined || getAt(model, ['thinking']) !== undefined
}

function writeModelReasoning(modelKey: string, enabled: boolean): void {
  let next = setAt(modelObject(modelKey), ['thinking'], undefined)
  next = setAt(next, ['reasoning'], enabled)
  commitModel(modelKey, next)
}

function resetModelReasoning(modelKey: string): void {
  let next = setAt(modelObject(modelKey), ['reasoning'], undefined)
  next = setAt(next, ['thinking'], undefined)
  commitModel(modelKey, next)
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
            @input="patchModelProse(modelKey, ['name'], ($event.target as HTMLInputElement).value)"
            @blur="patchModelString(modelKey, ['name'], ($event.target as HTMLInputElement).value)"
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
              @update:model-value="writeModelAttachment(modelKey, $event)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.providers.reasoning')"
          :can-reset="canResetReasoning(modelKey)"
          @reset="resetModelReasoning(modelKey)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="modelHasReasoning(modelKey)"
              :label="t('config.editor.providers.reasoning')"
              @update:model-value="writeModelReasoning(modelKey, $event)"
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

        <FormRow
          vertical
          :label="t('config.editor.providers.modelOptions')"
          :description="t('config.editor.providers.modelOptionsDesc')"
          :can-reset="canResetModelOptions(modelKey)"
          @reset="resetModelOptions(modelKey)"
        >
          <TextAreaField
            monospace
            :rows="3"
            :model-value="readModelOptionsText(modelKey)"
            :error="optionsDrafts.errorOf(modelKey)"
            :placeholder="MODEL_OPTIONS_PLACEHOLDER"
            :label="t('config.editor.providers.modelOptions')"
            @update:model-value="updateModelOptionsText(modelKey, $event)"
            @blur="optionsDrafts.blur(modelKey)"
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
              :error="variantDrafts.errorOf(variantDraftKey(modelKey, variantName))"
              :label="t('config.editor.providers.variantOptions')"
              @update:model-value="updateVariantText(modelKey, variantName, $event)"
              @blur="variantDrafts.blur(variantDraftKey(modelKey, variantName))"
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
