<script setup lang="ts">
// Agents configuration section: built-in overrides (`build`, `plan`, `general`, `explore`,
// `compaction`) with Customized badge & Reset override, plus custom agent CRUD.
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  BUILTIN_AGENTS,
  BUILTIN_AGENT_META,
  getAt,
  getBooleanAt,
  getNumberAt,
  getObjectAt,
  getStringAt,
  isPlainObject,
  setAt,
  type BuiltinAgentMeta,
  type JsonObject,
} from '../../../config/jsonPatch.ts'
import {
  configuredModelOptions,
  normalizeProseInput,
  normalizeTrimmedString,
  renameObjectKey,
} from '../../../config/sectionHelpers.ts'
import {
  useConfigSubEditorScope,
  useInlineRenameDrafts,
  useJsonObjectDrafts,
} from '../../../config/subEditorState.ts'
import type { ModelInfo } from '../../../proto/generated'
import Chevron from '../../Chevron.vue'
import DialogButton from '../../DialogButton.vue'
import Icon from '../../Icon.vue'
import TextButton from '../../TextButton.vue'
import ToggleSwitch from '../../ToggleSwitch.vue'
import {
  ColorSwatchPicker,
  FormRow,
  ModelField,
  NumberField,
  PermissionRulesEditor,
  SelectField,
  TextAreaField,
  ToolDelaysEditor,
} from '../index.ts'

const MODE_OPTIONS = [
  { value: 'primary', label: 'primary' },
  { value: 'subagent', label: 'subagent' },
  { value: 'all', label: 'all' },
] as const
const AGENT_OPTIONS_PLACEHOLDER = '{ "maxTokens": 8192 }'

const props = withDefaults(
  defineProps<{
    doc: JsonObject
    serverModels?: readonly ModelInfo[]
  }>(),
  {
    serverModels: () => [],
  },
)

const emit = defineEmits<{ 'update:doc': [next: JsonObject] }>()
const { t } = useI18n()

const openAgents = reactive(new Set<string>())
const newAgentName = ref('')
let lastEmittedDoc: JsonObject | undefined

const builtinSet = new Set<string>(BUILTIN_AGENTS)
const agentsMap = computed(() => getObjectAt(props.doc, ['agent']) ?? {})
const customAgentNames = computed(() => Object.keys(agentsMap.value).filter((name) => !builtinSet.has(name)))
const modelOptions = computed(() => configuredModelOptions(props.doc, props.serverModels))

function emitDoc(next: JsonObject): void {
  lastEmittedDoc = next
  emit('update:doc', next)
}

function isCustomized(name: string): boolean {
  return getObjectAt(props.doc, ['agent', name]) !== undefined
}

function agentObj(name: string): JsonObject {
  const raw = agentsMap.value[name]
  return isPlainObject(raw) ? raw : {}
}

const optionsDrafts = useJsonObjectDrafts({
  allowEmpty: true,
  invalidMessage: () => t('config.editor.providers.variantInvalidJson'),
})

const renames = useInlineRenameDrafts({
  exists: (key) => builtinSet.has(key) || Object.hasOwn(agentsMap.value, key),
  collisionMessage: () => t('config.editor.renameExists'),
  onCommit(oldName, target) {
    const renamed = renameObjectKey(agentsMap.value, oldName, target)
    if (openAgents.has(oldName)) {
      openAgents.delete(oldName)
      openAgents.add(target)
    }
    optionsDrafts.renameKey(oldName, target)
    emitDoc(setAt(props.doc, ['agent'], renamed))
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
    optionsDrafts.reconcile((name) => getObjectAt(agentObj(name), ['options']), forceClear)
    renames.reconcileKeys(customAgentNames.value, forceClear)
  },
)

if (subEditorScope.resetVersion) {
  watch(subEditorScope.resetVersion, () => {
    optionsDrafts.clearAll()
    renames.clearAll()
  })
}

function toggleAgent(name: string): void {
  if (openAgents.has(name)) openAgents.delete(name)
  else openAgents.add(name)
}

function commitAgent(name: string, nextAgent: JsonObject): void {
  const value = builtinSet.has(name) && Object.keys(nextAgent).length === 0 ? undefined : nextAgent
  emitDoc(setAt(props.doc, ['agent', name], value))
}

function patchAgent(name: string, subPath: readonly string[], value: unknown): void {
  commitAgent(name, setAt(agentObj(name), subPath, value))
}

function patchAgentProse(name: string, subPath: readonly string[], value: string): void {
  patchAgent(name, subPath, normalizeProseInput(value))
}

function patchAgentString(name: string, subPath: readonly string[], value: string): void {
  patchAgent(name, subPath, normalizeTrimmedString(value))
}

function patchPrompt(name: string, value: string): void {
  patchAgent(name, ['prompt'], value === '' ? undefined : value)
}

function resetBuiltin(name: string): void {
  openAgents.delete(name)
  optionsDrafts.remove(name)
  emitDoc(setAt(props.doc, ['agent', name], undefined))
}

function addCustomAgent(): void {
  const trimmed = newAgentName.value.trim()
  if (trimmed === '' || builtinSet.has(trimmed) || Object.hasOwn(agentsMap.value, trimmed)) return
  newAgentName.value = ''
  openAgents.add(trimmed)
  emitDoc(setAt(props.doc, ['agent', trimmed], { description: '' }))
}

function removeCustomAgent(name: string): void {
  openAgents.delete(name)
  renames.remove(name)
  optionsDrafts.remove(name)
  emitDoc(setAt(props.doc, ['agent', name], undefined))
}

function builtinMeta(name: string): BuiltinAgentMeta | undefined {
  return BUILTIN_AGENT_META.find((meta) => meta.name === name)
}

function readAgentCanSpawn(name: string, fallback: boolean): boolean {
  const obj = agentObj(name)
  return getBooleanAt(obj, ['can_spawn']) ?? getBooleanAt(obj, ['spawn']) ?? fallback
}

function canResetAgentCanSpawn(name: string): boolean {
  const obj = agentObj(name)
  return getAt(obj, ['can_spawn']) !== undefined || getAt(obj, ['spawn']) !== undefined
}

function writeAgentCanSpawn(name: string, enabled: boolean): void {
  let next = setAt(agentObj(name), ['spawn'], undefined)
  next = setAt(next, ['can_spawn'], enabled)
  commitAgent(name, next)
}

function resetAgentCanSpawn(name: string): void {
  let next = setAt(agentObj(name), ['can_spawn'], undefined)
  next = setAt(next, ['spawn'], undefined)
  commitAgent(name, next)
}

function readAgentDisabled(name: string): boolean {
  const obj = agentObj(name)
  return (getBooleanAt(obj, ['disable']) ?? getBooleanAt(obj, ['disabled'])) === true
}

function writeAgentDisabled(name: string, disabled: boolean): void {
  let next = setAt(agentObj(name), ['disabled'], undefined)
  next = setAt(next, ['disable'], disabled ? true : undefined)
  commitAgent(name, next)
}

function readAgentOptionsText(name: string): string {
  return optionsDrafts.read(name, getObjectAt(agentObj(name), ['options']))
}

function canResetAgentOptions(name: string): boolean {
  return getAt(agentObj(name), ['options']) !== undefined || optionsDrafts.hasDraft(name)
}

function updateAgentOptionsText(name: string, text: string): void {
  const currentOptions = getObjectAt(agentObj(name), ['options'])
  optionsDrafts.update(name, text, currentOptions, (parsed) => {
    patchAgent(name, ['options'], parsed)
  })
}

function resetAgentOptions(name: string): void {
  optionsDrafts.remove(name)
  patchAgent(name, ['options'], undefined)
}
</script>

<template>
  <div class="section-body">
    <h3 class="group-heading">{{ t('config.editor.agents.builtinHeading') }}</h3>

    <div v-for="meta in BUILTIN_AGENT_META" :key="meta.name" class="card">
      <div class="card-head">
        <button type="button" class="expand-btn" @click="toggleAgent(meta.name)">
          <Chevron :open="openAgents.has(meta.name)" :size="12" />
          <span class="card-id">{{ meta.name }}</span>
          <span class="meta-pill">{{ meta.mode }}</span>
          <span v-if="isCustomized(meta.name)" class="custom-badge">
            {{ t('config.editor.agents.customized') }}
          </span>
        </button>
        <TextButton
          v-if="isCustomized(meta.name)"
          :label="t('config.editor.agents.resetOverride')"
          @click="resetBuiltin(meta.name)"
        />
      </div>

      <div v-if="openAgents.has(meta.name)" class="card-body">
        <FormRow :label="t('config.editor.agents.description')">
          <input
            :value="getStringAt(agentObj(meta.name), ['description']) ?? ''"
            type="text"
            class="text-input"
            @input="patchAgentProse(meta.name, ['description'], ($event.target as HTMLInputElement).value)"
            @blur="patchAgentString(meta.name, ['description'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.mode')"
          :can-reset="getAt(agentObj(meta.name), ['mode']) !== undefined"
          @reset="patchAgent(meta.name, ['mode'], undefined)"
        >
          <SelectField
            :model-value="getStringAt(agentObj(meta.name), ['mode']) ?? ''"
            :options="MODE_OPTIONS"
            :placeholder="meta.mode"
            :label="t('config.editor.agents.mode')"
            @update:model-value="patchAgentString(meta.name, ['mode'], $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.model')"
          :can-reset="getAt(agentObj(meta.name), ['model']) !== undefined"
          @reset="patchAgent(meta.name, ['model'], undefined)"
        >
          <ModelField
            :model-value="getStringAt(agentObj(meta.name), ['model']) ?? ''"
            :options="modelOptions"
            :label="t('config.editor.agents.model')"
            @update:model-value="patchAgentString(meta.name, ['model'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.variant')">
          <input
            :value="getStringAt(agentObj(meta.name), ['variant']) ?? ''"
            type="text"
            class="text-input mono"
            spellcheck="false"
            @input="patchAgentString(meta.name, ['variant'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.color')">
          <ColorSwatchPicker
            :model-value="getStringAt(agentObj(meta.name), ['color']) ?? ''"
            :label="t('config.editor.agents.color')"
            @update:model-value="patchAgentString(meta.name, ['color'], $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.creatable')"
          :can-reset="getAt(agentObj(meta.name), ['creatable']) !== undefined"
          @reset="patchAgent(meta.name, ['creatable'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(meta.name), ['creatable']) ?? !meta.hidden"
              :label="t('config.editor.agents.creatable')"
              @update:model-value="patchAgent(meta.name, ['creatable'], $event)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.canSpawn')"
          :can-reset="canResetAgentCanSpawn(meta.name)"
          @reset="resetAgentCanSpawn(meta.name)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="readAgentCanSpawn(meta.name, meta.canSpawnDefault)"
              :label="t('config.editor.agents.canSpawn')"
              @update:model-value="writeAgentCanSpawn(meta.name, $event)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.hidden')"
          :can-reset="getAt(agentObj(meta.name), ['hidden']) !== undefined"
          @reset="patchAgent(meta.name, ['hidden'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(meta.name), ['hidden']) ?? meta.hidden"
              :label="t('config.editor.agents.hidden')"
              @update:model-value="patchAgent(meta.name, ['hidden'], $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.agents.disable')">
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="readAgentDisabled(meta.name)"
              :label="t('config.editor.agents.disable')"
              @update:model-value="writeAgentDisabled(meta.name, $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.providers.temperature')">
          <NumberField
            :model-value="getNumberAt(agentObj(meta.name), ['temperature'])"
            :min="0"
            :max="2"
            :step="0.1"
            @update:model-value="patchAgent(meta.name, ['temperature'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.topP')">
          <NumberField
            :model-value="getNumberAt(agentObj(meta.name), ['top_p'])"
            :min="0"
            :max="1"
            :step="0.05"
            @update:model-value="patchAgent(meta.name, ['top_p'], $event)"
          />
        </FormRow>

        <FormRow
          vertical
          :label="t('config.editor.agents.options')"
          :description="t('config.editor.agents.optionsDesc')"
          :can-reset="canResetAgentOptions(meta.name)"
          @reset="resetAgentOptions(meta.name)"
        >
          <TextAreaField
            monospace
            :rows="3"
            :model-value="readAgentOptionsText(meta.name)"
            :error="optionsDrafts.errorOf(meta.name)"
            :placeholder="AGENT_OPTIONS_PLACEHOLDER"
            :label="t('config.editor.agents.options')"
            @update:model-value="updateAgentOptionsText(meta.name, $event)"
            @blur="optionsDrafts.blur(meta.name)"
          />
        </FormRow>
        <FormRow vertical :label="t('config.editor.agents.prompt')">
          <TextAreaField
            :rows="5"
            :model-value="getStringAt(agentObj(meta.name), ['prompt']) ?? ''"
            :placeholder="t('config.editor.agents.promptPlaceholder')"
            :label="t('config.editor.agents.prompt')"
            @update:model-value="patchPrompt(meta.name, $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.permission')">
          <PermissionRulesEditor
            :model-value="getAt(agentObj(meta.name), ['permission'])"
            :label="t('config.editor.agents.permission')"
            @update:model-value="patchAgent(meta.name, ['permission'], $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.delay')">
          <ToolDelaysEditor
            :model-value="getAt(agentObj(meta.name), ['delay'])"
            @update:model-value="patchAgent(meta.name, ['delay'], $event)"
          />
        </FormRow>
      </div>
    </div>

    <h3 class="group-heading divider">{{ t('config.editor.agents.customHeading') }}</h3>

    <p v-if="customAgentNames.length === 0" class="empty">{{ t('config.editor.agents.customEmpty') }}</p>

    <div v-for="name in customAgentNames" :key="name" class="card">
      <div class="card-head">
        <button type="button" class="expand-btn" @click="toggleAgent(name)">
          <Chevron :open="openAgents.has(name)" :size="12" />
          <span class="card-id">{{ name }}</span>
          <span v-if="getStringAt(agentObj(name), ['description'])" class="card-sub">
            {{ getStringAt(agentObj(name), ['description']) }}
          </span>
        </button>
        <button
          type="button"
          class="icon-btn danger"
          :aria-label="t('config.editor.delete')"
          :title="t('config.editor.delete')"
          @click="removeCustomAgent(name)"
        >
          <Icon icon="trash-16" />
        </button>
      </div>

      <div v-if="openAgents.has(name)" class="card-body">
        <FormRow :label="t('config.editor.rename')" :error="renames.errorOf(name)">
          <div class="rename-row">
            <input
              :value="renames.read(name)"
              type="text"
              class="text-input mono"
              spellcheck="false"
              @input="renames.set(name, ($event.target as HTMLInputElement).value)"
              @blur="renames.blur(name)"
              @keydown.enter.prevent="!$event.isComposing && renames.commit(name)"
            />
            <DialogButton
              small
              :label="t('config.editor.rename')"
              :disabled="!renames.canCommit(name)"
              @mousedown.prevent
              @click="renames.commit(name)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.agents.description')">
          <input
            :value="getStringAt(agentObj(name), ['description']) ?? ''"
            type="text"
            class="text-input"
            @input="patchAgentProse(name, ['description'], ($event.target as HTMLInputElement).value)"
            @blur="patchAgentString(name, ['description'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.mode')"
          :can-reset="getAt(agentObj(name), ['mode']) !== undefined"
          @reset="patchAgent(name, ['mode'], undefined)"
        >
          <SelectField
            :model-value="getStringAt(agentObj(name), ['mode']) ?? ''"
            :options="MODE_OPTIONS"
            :placeholder="builtinMeta(name)?.mode ?? 'all'"
            :label="t('config.editor.agents.mode')"
            @update:model-value="patchAgentString(name, ['mode'], $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.model')"
          :can-reset="getAt(agentObj(name), ['model']) !== undefined"
          @reset="patchAgent(name, ['model'], undefined)"
        >
          <ModelField
            :model-value="getStringAt(agentObj(name), ['model']) ?? ''"
            :options="modelOptions"
            :label="t('config.editor.agents.model')"
            @update:model-value="patchAgentString(name, ['model'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.variant')">
          <input
            :value="getStringAt(agentObj(name), ['variant']) ?? ''"
            type="text"
            class="text-input mono"
            spellcheck="false"
            @input="patchAgentString(name, ['variant'], ($event.target as HTMLInputElement).value)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.agents.color')">
          <ColorSwatchPicker
            :model-value="getStringAt(agentObj(name), ['color']) ?? ''"
            :label="t('config.editor.agents.color')"
            @update:model-value="patchAgentString(name, ['color'], $event)"
          />
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.creatable')"
          :can-reset="getAt(agentObj(name), ['creatable']) !== undefined"
          @reset="patchAgent(name, ['creatable'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(name), ['creatable']) ?? true"
              :label="t('config.editor.agents.creatable')"
              @update:model-value="patchAgent(name, ['creatable'], $event)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.canSpawn')"
          :can-reset="canResetAgentCanSpawn(name)"
          @reset="resetAgentCanSpawn(name)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="readAgentCanSpawn(name, false)"
              :label="t('config.editor.agents.canSpawn')"
              @update:model-value="writeAgentCanSpawn(name, $event)"
            />
          </div>
        </FormRow>

        <FormRow
          :label="t('config.editor.agents.hidden')"
          :can-reset="getAt(agentObj(name), ['hidden']) !== undefined"
          @reset="patchAgent(name, ['hidden'], undefined)"
        >
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="getBooleanAt(agentObj(name), ['hidden']) ?? false"
              :label="t('config.editor.agents.hidden')"
              @update:model-value="patchAgent(name, ['hidden'], $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.agents.disable')">
          <div class="switch-wrap">
            <ToggleSwitch
              :model-value="readAgentDisabled(name)"
              :label="t('config.editor.agents.disable')"
              @update:model-value="writeAgentDisabled(name, $event)"
            />
          </div>
        </FormRow>

        <FormRow :label="t('config.editor.providers.temperature')">
          <NumberField
            :model-value="getNumberAt(agentObj(name), ['temperature'])"
            :min="0"
            :max="2"
            :step="0.1"
            @update:model-value="patchAgent(name, ['temperature'], $event)"
          />
        </FormRow>

        <FormRow :label="t('config.editor.providers.topP')">
          <NumberField
            :model-value="getNumberAt(agentObj(name), ['top_p'])"
            :min="0"
            :max="1"
            :step="0.05"
            @update:model-value="patchAgent(name, ['top_p'], $event)"
          />
        </FormRow>

        <FormRow
          vertical
          :label="t('config.editor.agents.options')"
          :description="t('config.editor.agents.optionsDesc')"
          :can-reset="canResetAgentOptions(name)"
          @reset="resetAgentOptions(name)"
        >
          <TextAreaField
            monospace
            :rows="3"
            :model-value="readAgentOptionsText(name)"
            :error="optionsDrafts.errorOf(name)"
            :placeholder="AGENT_OPTIONS_PLACEHOLDER"
            :label="t('config.editor.agents.options')"
            @update:model-value="updateAgentOptionsText(name, $event)"
            @blur="optionsDrafts.blur(name)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.prompt')">
          <TextAreaField
            :rows="5"
            :model-value="getStringAt(agentObj(name), ['prompt']) ?? ''"
            :placeholder="t('config.editor.agents.promptPlaceholder')"
            :label="t('config.editor.agents.prompt')"
            @update:model-value="patchPrompt(name, $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.permission')">
          <PermissionRulesEditor
            :model-value="getAt(agentObj(name), ['permission'])"
            :label="t('config.editor.agents.permission')"
            @update:model-value="patchAgent(name, ['permission'], $event)"
          />
        </FormRow>

        <FormRow vertical :label="t('config.editor.agents.delay')">
          <ToolDelaysEditor
            :model-value="getAt(agentObj(name), ['delay'])"
            @update:model-value="patchAgent(name, ['delay'], $event)"
          />
        </FormRow>
      </div>
    </div>

    <div class="add-inline">
      <input
        v-model="newAgentName"
        type="text"
        class="text-input mono"
        spellcheck="false"
        :placeholder="t('config.editor.agents.agentNamePlaceholder')"
        :aria-label="t('config.editor.agents.agentNamePlaceholder')"
        @keydown.enter.prevent="!$event.isComposing && addCustomAgent()"
      />
      <DialogButton
        small
        kind="primary"
        :label="t('config.editor.agents.addAgent')"
        :disabled="
          newAgentName.trim() === '' ||
          builtinSet.has(newAgentName.trim()) ||
          Object.hasOwn(agentsMap, newAgentName.trim())
        "
        @click="addCustomAgent"
      />
    </div>
  </div>
</template>

<style scoped>
.section-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.group-heading {
  margin: 0 0 2px;
  color: var(--text-muted);
  font-size: var(--small-font-size);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.group-heading.divider {
  margin-top: 12px;
  padding-top: 14px;
  border-top: 1px solid var(--border);
}

.empty {
  margin: 2px 0;
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
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta-pill {
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--surface);
  color: var(--text-muted);
  font-size: var(--micro-font-size);
}

.custom-badge {
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--user-bubble);
  box-shadow: inset 0 0 0 1px var(--user-bubble-border);
  color: var(--accent-text);
  font-size: var(--micro-font-size);
  font-weight: 500;
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

.add-inline {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
}

.add-inline .text-input {
  flex: 1;
}
</style>
